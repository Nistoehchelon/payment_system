use cryptocurrencies_stuff::currencies::LTCAtomicUnit;
use payment_system_structs::{Payment, PaymentSystemConfig};

pub async fn check_payment(
    db_pool: sqlx::PgPool,
    payment: &Payment,
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