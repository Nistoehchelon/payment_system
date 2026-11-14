
use anyhow::{Context, anyhow};
use bitcoin::{
    ScriptBuf, Transaction, TxIn, TxOut, Witness, absolute::LockTime, transaction::Version,
};
use cryptocurrencies_stuff::currencies::LTCAtomicUnit;

pub mod check_payment;
pub use check_payment::check_payment;
pub mod get_address_balance;
pub use get_address_balance::get_address_balance_for_small_payments;
pub mod get_utxos;
pub mod transfer_to_treasury_wallet_after_receiving_payment;
pub use transfer_to_treasury_wallet_after_receiving_payment::sweep_balance;

use serde::Deserialize;

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
