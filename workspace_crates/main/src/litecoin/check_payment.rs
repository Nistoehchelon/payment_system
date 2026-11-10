use cryptocurrencies_stuff::currencies::LTCAtomicUnit;
use payment_system_structs::ReceivingPaymentAddress;

use crate::litecoin::get_address_balance::get_address_balance;

pub async fn check_payment(
    payment_receiver_address: &ReceivingPaymentAddress,
    required_amount: LTCAtomicUnit,
) -> anyhow::Result<bool> {

    let min_confirmations = 1;
    let balance = get_address_balance(payment_receiver_address.clone(), min_confirmations).await?;
    // step 3: if balance is not sufficient, return false
    let answer = balance >= required_amount;
    Ok(answer)
}
