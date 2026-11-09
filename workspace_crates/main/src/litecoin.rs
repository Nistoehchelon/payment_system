use anyhow::{Context, anyhow, bail};
use bitcoin::{
    Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness,
    absolute::LockTime,
    address::{Address as BitcoinAddress, NetworkChecked, NetworkUnchecked},
    consensus::encode::serialize_hex,
    hex::HexToArrayError,
    p2p::Address,
    transaction::Version,
};
use cryptocurrencies_stuff::{currencies::LTCAtomicUnit, derive_ltc::derive_ltc_bip84_address};
use payment_system_structs::{PaymentSqlxShape, PaymentSystemConfig};

pub async fn check_payment(
    _db_pool: sqlx::PgPool,
    payment: &PaymentSqlxShape,
    client: &reqwest::Client,
    payment_system_config: &PaymentSystemConfig,
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
    db_pool: sqlx::PgPool,
    master_seed: String,
    from_index: u64,
    to_address: payment_system_structs::Address,
) -> anyhow::Result<String> {
    let from_address =
        derive_ltc_bip84_address(&master_seed, from_index).map_err(|e| anyhow!(e))?;
    let from_balance = get_balance(&from_address).await?;
    let fee_estimation_per_virtual_byte =
        get_a_fee_estimate_per_virtual_byte(RecommendedFeeType::FastestFee).await?;
    // todo!();
    // let prepared_transaction = prepare_sweep_tx().await?;
    // let estimated_fee = estimate_transaction_fee_based_tx_bytes(
    //     fee_estimation_per_virtual_byte,
    //     prepared_transaction.clone(),
    // )
    // .await?;
    // let max_possible_sweep_amount = match from_balance.checked_sub_result(estimated_fee.clone()) {
    //     Ok(amount) => amount,
    //     Err(e) => {
    //         bail!("{e:?}")
    //     }
    // };

    // let _ = send_transaction(
    //     master_seed,
    //     from_index,
    //     max_possible_sweep_amount,
    //     estimated_fee,
    //     to_address,
    // )
    // .await?;
    // // send and mark as complete in db

    todo!()
}
pub async fn send_transaction(
    master_seed: String,
    index: u64,
    amount: LTCAtomicUnit,
    fee: LTCAtomicUnit,
    destination: Address,
) -> anyhow::Result<String> {
    todo!()
}
#[derive(Debug, serde::Deserialize)]
pub struct Utxo {
    pub txid: String,
    pub vout: u32,
    pub value: u64,
    pub status: UtxoStatus,
}

#[derive(Debug, serde::Deserialize)]
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

pub async fn prepare_sweep_tx(
    utxos: Vec<Utxo>,
    destination: payment_system_structs::Address,
    fee_per_vbyte: LTCAtomicUnit,
) -> anyhow::Result<String> {
    if utxos.is_empty() {
        bail!("No UTXOs to sweep");
    }
    let dest: BitcoinAddress<NetworkUnchecked> =
        destination.0.parse()?;
    let dest: BitcoinAddress<NetworkChecked> = dest.require_network(bitcoin::Network::Bitcoin)?;
    let dest_script = dest.script_pubkey();

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
    let total_in: u64 = utxos.iter().map(|u| u.value).sum();
    let arbitrary_number_one = 10;
    let arbitrary_number_two = 68;
    let estimated_vsize = arbitrary_number_one
        + (utxos.len() as u64 * arbitrary_number_two)
        + dest_script.len() as u64;
    let fee = fee_per_vbyte
        .checked_mul_result(LTCAtomicUnit::from_atomic(estimated_vsize))
        .or_else(|e| bail!("fee overflow"))?;
    let output_value = total_in
        .checked_sub(fee.as_atomic())
        .ok_or_else(|| anyhow!("balance {} <  estimated fee {}", total_in, fee.as_atomic()))?;
    let tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: inputs,
        output: vec![TxOut {
            value: Amount::from_sat(output_value),
            script_pubkey: dest_script,
        }],
    };
    Ok(serialize_hex(&tx))
}
pub async fn estimate_transaction_fee_based_tx_bytes(
    fee_per_vbyte: LTCAtomicUnit,
    tx: String,
) -> anyhow::Result<LTCAtomicUnit> {
    todo!()
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
