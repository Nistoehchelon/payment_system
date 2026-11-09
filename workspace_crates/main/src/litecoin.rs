use cryptocurrencies_stuff::{atomic_unit::AtomicUnit, currencies::LTCAtomicUnit, derive_ltc::derive_ltc_bip84_address};
use payment_system_structs::{PaymentSqlxShape, PaymentSystemConfig};

pub async fn check_payment(
    db_pool: sqlx::PgPool,
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
    let answer =  balance >= required_amount;
    Ok(answer)

}

async fn get_balance(_address: &str) -> anyhow::Result<LTCAtomicUnit> {
    // TODO: Contact an API, say, Litecoinspace and filter results... Either for confirmed balance or any...
    todo!()
}

pub async fn sweep_balance(db_pool: sqlx::PgPool, master_seed: String, from_index: u64, to_address: String) -> anyhow::Result<String>{
    let from_address = derive_ltc_bip84_address(&master_seed, from_index)?;
    let from_balance = get_balance(&from_address).await?;
    let fee_estimation_per_virtual_byte = get_a_fee_estimate_per_virtual_byte().await?;
    let prepared_transaction = prepare_sweep_tx().await?;
    let estimated_fee = estimate_transaction_fee_based_tx_bytes(fee_estimation_per_virtual_byte, prepared_transaction.clone()).await?;
    let max_possible_sweep_amount = from_balance - estimated_fee;
    
    let _ = send_transaction().await?;
    // send and mark as complete in db

    todo!()
}
pub async fn send_transaction(fee: LTCAtomicUnit, )
pub async fn prepare_sweep_tx() -> anyhow::Result<String> {
todo!()
}
pub async fn estimate_transaction_fee_based_tx_bytes(fee_per_vbyte: LTCAtomicUnit, tx: String) -> anyhow::Result<LTCAtomicUnit> {
    todo!()
}

pub async fn get_a_fee_estimate_per_virtual_byte() -> anyhow::Result<LTCAtomicUnit> {
todo!()
}