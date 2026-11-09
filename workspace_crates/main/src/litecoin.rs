use anyhow::{Context, anyhow, bail};
use cryptocurrencies_stuff::{currencies::LTCAtomicUnit, derive_ltc::derive_ltc_bip84_address};
use payment_system_structs::{Address, PaymentSqlxShape, PaymentSystemConfig};

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
    to_address: Address,
) -> anyhow::Result<String> {
    let from_address =
        derive_ltc_bip84_address(&master_seed, from_index).map_err(|e| anyhow!(e))?;
    let from_balance = get_balance(&from_address).await?;
    let fee_estimation_per_virtual_byte = get_a_fee_estimate_per_virtual_byte().await?;
    let prepared_transaction = prepare_sweep_tx().await?;
    let estimated_fee = estimate_transaction_fee_based_tx_bytes(
        fee_estimation_per_virtual_byte,
        prepared_transaction.clone(),
    )
    .await?;
    let max_possible_sweep_amount = match from_balance.checked_sub_result(estimated_fee.clone()) {
        Ok(amount) => amount,
        Err(e) => {
            bail!("{e:?}")
        }
    };

    let _ = send_transaction(
        master_seed,
        from_index,
        max_possible_sweep_amount,
        estimated_fee,
        to_address,
    )
    .await?;
    // send and mark as complete in db

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
pub async fn prepare_sweep_tx() -> anyhow::Result<String> {
    todo!()
}
pub async fn estimate_transaction_fee_based_tx_bytes(
    fee_per_vbyte: LTCAtomicUnit,
    tx: String,
) -> anyhow::Result<LTCAtomicUnit> {
    todo!()
}

pub async fn get_a_fee_estimate_per_virtual_byte() -> anyhow::Result<LTCAtomicUnit> {
    todo!()
}
