use anyhow::{Context, bail};
use bitcoin::bip32::{DerivationPath, Xpriv};
use bitcoin::consensus::serialize;
use bitcoin::ecdsa::Signature as EcdsaSig;
use bitcoin::hashes::Hash;
use bitcoin::key::PrivateKey;
use bitcoin::secp256k1::{Message, Secp256k1};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{ScriptBuf, Witness};
use std::str::FromStr;

use crate::litecoin::PreparedSweep;

pub struct SignedTransaction(pub bitcoin::Transaction);
pub async fn sign_sweep_transaction(
    master_seed: &str,
    from_index: u64,
    prepared: PreparedSweep,
) -> anyhow::Result<SignedTransaction> {
    let secp = Secp256k1::new();

    use bip39::Mnemonic;
    let mnemonic = Mnemonic::parse(master_seed)?;
    let seed = mnemonic.to_seed("");
    let root = Xpriv::new_master(bitcoin::Network::Bitcoin, &seed)?;
    let path = DerivationPath::from_str(&format!("m/84'/2'/0'/0/{from_index}"))?;
    let child = root.derive_priv(&secp, &path)?;
    let privkey = PrivateKey::new(child.private_key, bitcoin::Network::Bitcoin);
    let pubkey = privkey.public_key(&secp);

    let mut tx = prepared.tx;
    let mut witnesses = Vec::with_capacity(tx.input.len());
    {
        let mut cache = SighashCache::new(&tx);
        for (i, _txin) in tx.input.iter().enumerate() {
            let sighash = cache.p2wpkh_signature_hash(
                i,
                &prepared.from_script_pubkey,
                prepared.input_values[i],
                EcdsaSighashType::All,
            )?;
            let msg = Message::from_digest(sighash.to_byte_array());
            let sig = secp.sign_ecdsa(&msg, &privkey.inner);

            let mut witness = Witness::new();
            witness.push(
                EcdsaSig {
                    signature: sig,
                    sighash_type: EcdsaSighashType::All,
                }
                .to_vec(),
            );
            witness.push(pubkey.to_bytes());
            witnesses.push(witness);
        }
    }
    for (txin, witness) in tx.input.iter_mut().zip(witnesses) {
        txin.witness = witness;
        txin.script_sig = ScriptBuf::new();
    }

    Ok(SignedTransaction(tx))
}

pub async fn broadcast_tx(tx: SignedTransaction) -> anyhow::Result<String> {
    let tx_hex = hex::encode(serialize(&tx.0));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let resp = client
        .post("https://litecoinspace.org/api/tx")
        .header("Content-Type", "text/plain")
        .body(tx_hex)
        .send()
        .await
        .context("broadcast request failed")?;

    let status = resp.status();
    let body = resp.text().await.context("failed to read broadcast body")?;

    if !status.is_success() {
        
        bail!("broadcast rejected ({}): {}", status, body.trim());
    }
    Ok(body.trim().to_string())
}
