use anyhow::{Context, anyhow, bail};
use bitcoin::bip32::{DerivationPath, Xpriv};
use bitcoin::consensus::serialize;
use bitcoin::ecdsa::Signature as EcdsaSig;
use bitcoin::hashes::Hash;
use bitcoin::key::PrivateKey;
use bitcoin::secp256k1::{Message, Secp256k1};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{ScriptBuf, Witness};
use std::str::FromStr;

use crate::litecoin::{calculate_fee, estimate_vsize};

#[derive(Debug, Clone)]
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

pub async fn sweep_balance(
    master_seed: String,
    from_index: u64,
    to_address: payment_system_structs::CryptoAddress,
) -> anyhow::Result<()> {
    let from_address = payment_system_structs::CryptoAddress(
        cryptocurrencies_stuff::derive_ltc::derive_ltc_bip84_address(&master_seed, from_index)
            .map_err(|e| anyhow!(e))?,
    );
    let fee_estimation_per_virtual_byte = crate::litecoin::get_a_fee_estimate_per_virtual_byte(
        crate::litecoin::RecommendedFeeType::FastestFee,
    )
    .await?;
    let utxos = crate::litecoin::get_utxos::get_utxos(&from_address.0)
        .await?
        .into_iter()
        .filter(|u| u.status.confirmed)
        .collect();
    let prepared_transaction = prepare_sweep_tx(
        utxos,
        from_address,
        to_address,
        fee_estimation_per_virtual_byte,
    )
    .await?;

    let signed_tx = sign_sweep_transaction(&master_seed, from_index, prepared_transaction).await?;
    let res = broadcast_tx(signed_tx).await;
    tracing::debug!("Res: {res:?}");
    Ok(())
}

#[derive(Debug, Clone)]
pub struct PreparedSweep {
    pub tx: bitcoin::blockdata::transaction::Transaction,
    pub input_values: Vec<bitcoin::Amount>,
    pub from_script_pubkey: ScriptBuf,
}

pub async fn prepare_sweep_tx(
    utxos: Vec<super::get_utxos::Utxo>,
    origin: payment_system_structs::CryptoAddress,
    destination: payment_system_structs::CryptoAddress,
    fee_per_vbyte: cryptocurrencies_stuff::currencies::LTCAtomicUnit,
) -> anyhow::Result<PreparedSweep> {
    if utxos.is_empty() {
        bail!("No UTXOs to sweep");
    }
    let script_bytes = outscript::parse_bitcoin_based_address("litecoin", &destination.0)
        .map_err(|e| anyhow!("invalid Litecoin address: {e}"))?;
    let dest_script = ScriptBuf::from_bytes(script_bytes.bytes().to_vec());

    let inputs: Vec<bitcoin::TxIn> = utxos
        .iter()
        .map(|u| -> anyhow::Result<bitcoin::TxIn> {
            let txid = u
                .txid
                .parse()
                .with_context(|| format!("API returned invalid txid: {:?}", u.txid))?;
            Ok(bitcoin::TxIn {
                previous_output: bitcoin::OutPoint { txid, vout: u.vout },
                script_sig: ScriptBuf::new(),
                sequence: bitcoin::Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::new(),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let estimated_vsize = estimate_vsize(&inputs, &dest_script);
    let estimated_fee = calculate_fee(fee_per_vbyte, estimated_vsize)?;
    tracing::debug!("Estimated fee: {}", estimated_fee.as_atomic());

    let total_in: u64 = utxos.iter().map(|u| u.value).sum();
    let output_value = total_in
        .checked_sub(estimated_fee.as_atomic())
        .ok_or_else(|| {
            anyhow!(
                "balance {} <  estimated fee {}",
                total_in,
                estimated_fee.as_atomic()
            )
        })?;
    tracing::debug!("Output value: {output_value}");
    let tx = bitcoin::Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: inputs,
        output: vec![bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(output_value),
            script_pubkey: dest_script,
        }],
    };
    let from_script_bytes = outscript::parse_bitcoin_based_address("litecoin", &origin.0)
        .map_err(|e| anyhow::anyhow!("invalid source Litecoin address: {e}"))?;
    let from_script_pubkey = ScriptBuf::from_bytes(from_script_bytes.bytes().to_vec());
    Ok(PreparedSweep {
        tx,
        input_values: utxos
            .iter()
            .map(|u| bitcoin::Amount::from_sat(u.value))
            .collect(),
        from_script_pubkey,
    })
}
