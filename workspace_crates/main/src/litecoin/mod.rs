pub mod reverse;
pub mod sign_and_send_transaction;
use anyhow::{Context, anyhow, bail};
use bitcoin::{
    Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness, absolute::LockTime,
    transaction::Version,
};
use cryptocurrencies_stuff::{currencies::LTCAtomicUnit, derive_ltc::derive_ltc_bip84_address};
use outscript::parse_bitcoin_based_address;
use payment_system_structs::PaymentSqlxShape;

pub async fn check_payment(
    _db_pool: sqlx::PgPool,
    payment: &PaymentSqlxShape,
) -> anyhow::Result<bool> {
    // step 1: get the address where I want the user to pay
    let payment_address = payment.address.clone();
    // step 2: receive the balance of the litecoin address
    let balance = get_balance(&payment_address).await?;

    let required_amount: u64 = payment.amount.try_into()?;
    let required_amount = LTCAtomicUnit::from_atomic(required_amount);
    // step 3: if balance is not sufficient, return false
    let answer = balance >= required_amount;
    Ok(answer)
}

use serde::Deserialize;

use crate::litecoin::sign_and_send_transaction::{broadcast_tx, sign_sweep_transaction};

#[derive(Debug, Deserialize)]
struct AddressStats {
    chain_stats: ChainStats,
}

#[derive(Debug, Deserialize)]
struct ChainStats {
    funded_txo_sum: u64,
    spent_txo_sum: u64,
}

pub async fn get_balance(address: &str) -> anyhow::Result<LTCAtomicUnit> {
    let url = format!("https://litecoinspace.org/api/address/{address}");
    let stats: AddressStats = reqwest::get(&url)
        .await
        .context("failed to reach Litecoin Space")?
        .error_for_status()
        .context("address not found or API error")?
        .json()
        .await
        .context("failed to parse address stats")?;

    let confirmed = stats
        .chain_stats
        .funded_txo_sum
        .checked_sub(stats.chain_stats.spent_txo_sum)
        .ok_or_else(|| anyhow!("address stats are inconsistent (spent > funded)"))?;

    Ok(LTCAtomicUnit::from_atomic(confirmed))
}

pub async fn sweep_balance(
    _db_pool: sqlx::PgPool,
    master_seed: String,
    from_index: u64,
    to_address: payment_system_structs::Address,
) -> anyhow::Result<String> {
    let from_address = payment_system_structs::Address(
        derive_ltc_bip84_address(&master_seed, from_index).map_err(|e| anyhow!(e))?,
    );
    let fee_estimation_per_virtual_byte =
        get_a_fee_estimate_per_virtual_byte(RecommendedFeeType::FastestFee).await?;
    let utxos = get_utxos(&from_address.0).await?;
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
    Ok("TX Sent".to_string())
}
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Utxo {
    pub txid: String,
    pub vout: u32,
    pub value: u64,
    pub status: UtxoStatus,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct UtxoStatus {
    pub confirmed: bool,
}

pub async fn get_utxos(address: &str) -> anyhow::Result<Vec<Utxo>> {
    let url = format!("https://litecoinspace.org/api/address/{address}/utxo");
    let utxos: Vec<Utxo> = reqwest::get(&url)
        .await
        .context("failed to reach UTXO endpoint")?
        .error_for_status()
        .context("UTXO endpoint returned an error")?
        .json()
        .await
        .context("failed to parse UTXOs")?;

    // Only use confirmed UTXOs to avoid spending unconfirmed outputs
    Ok(utxos.into_iter().filter(|u| u.status.confirmed).collect())
}

#[derive(Debug, Clone)]
pub struct PreparedSweep {
    pub tx: bitcoin::blockdata::transaction::Transaction,
    pub input_values: Vec<bitcoin::Amount>,
    pub from_script_pubkey: ScriptBuf,
}

pub async fn prepare_sweep_tx(
    utxos: Vec<Utxo>,
    origin: payment_system_structs::Address,
    destination: payment_system_structs::Address,
    fee_per_vbyte: LTCAtomicUnit,
) -> anyhow::Result<PreparedSweep> {
    if utxos.is_empty() {
        bail!("No UTXOs to sweep");
    }
    let script_bytes = parse_bitcoin_based_address("litecoin", &destination.0)
        .map_err(|e| anyhow!("invalid Litecoin address: {e}"))?;
    let dest_script = ScriptBuf::from_bytes(script_bytes.bytes().to_vec());

    let inputs: Vec<TxIn> = utxos
        .iter()
        .map(|u| -> anyhow::Result<TxIn> {
            let txid = u
                .txid
                .parse()
                .with_context(|| format!("API returned invalid txid: {:?}", u.txid))?;
            Ok(TxIn {
                previous_output: OutPoint { txid, vout: u.vout },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
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
    let tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: inputs,
        output: vec![TxOut {
            value: Amount::from_sat(output_value),
            script_pubkey: dest_script,
        }],
    };
    let from_script_bytes = parse_bitcoin_based_address("litecoin", &origin.0)
        .map_err(|e| anyhow!("invalid source Litecoin address: {e}"))?;
    let from_script_pubkey = ScriptBuf::from_bytes(from_script_bytes.bytes().to_vec());
    Ok(PreparedSweep {
        tx,
        input_values: utxos.iter().map(|u| Amount::from_sat(u.value)).collect(),
        from_script_pubkey: from_script_pubkey,
    })
}

pub fn calculate_fee(fee_per_vbyte: LTCAtomicUnit, vsize: u64) -> anyhow::Result<LTCAtomicUnit> {
    fee_per_vbyte
        .checked_mul_result(LTCAtomicUnit::from_atomic(vsize))
        .map_err(|e| anyhow!("fee overflow: {e:?}"))
}

pub fn estimate_vsize(real_inputs: &[TxIn], dest_script: &ScriptBuf) -> u64 {
    let dummy_inputs: Vec<TxIn> = real_inputs
        .iter()
        .map(|txin| {
            let mut dummy_txin = txin.clone();
            let mut witness = Witness::new();

            witness.push(vec![0u8; 72]);
            witness.push(vec![0u8; 33]);

            dummy_txin.witness = witness;
            dummy_txin
        })
        .collect();
    let dummy_output = TxOut {
        value: bitcoin::Amount::from_sat(0),
        script_pubkey: dest_script.clone(),
    };

    let dummy_tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: dummy_inputs,
        output: vec![dummy_output],
    };
    dummy_tx.weight().to_vbytes_ceil() as u64
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendedFees {
    pub fastest_fee: u64,
    pub half_hour_fee: u64,
    pub hour_fee: u64,
    pub economy_fee: u64,
    pub minimum_fee: u64,
}
#[derive(Debug, Deserialize)]
pub enum RecommendedFeeType {
    FastestFee,
    HalfHourFee,
    HourFee,
    EconomyFee,
    MinimumFee,
}

pub async fn get_a_fee_estimate_per_virtual_byte(
    fee_type: RecommendedFeeType,
) -> anyhow::Result<LTCAtomicUnit> {
    let url = "https://litecoinspace.org/api/v1/fees/recommended";
    let fees: RecommendedFees = reqwest::get(url)
        .await
        .context("failed to reach fee endpoint")?
        .error_for_status()
        .context("fee endpoint returned an error")?
        .json()
        .await
        .context("failed to parse fee response")?;

    let fee = match fee_type {
        RecommendedFeeType::FastestFee => fees.fastest_fee,
        RecommendedFeeType::HalfHourFee => fees.half_hour_fee,
        RecommendedFeeType::HourFee => fees.hour_fee,
        RecommendedFeeType::EconomyFee => fees.economy_fee,
        RecommendedFeeType::MinimumFee => fees.minimum_fee,
    };
    Ok(LTCAtomicUnit::from_atomic(fee))
}
