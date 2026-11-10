use main::litecoin::{self, transfer_to_treasury_wallet_after_receiving_payment::prepare_sweep_tx};
use payment_system_structs::ReceivingPaymentAddress;

// Apparently tracing initialization is needed in tests
// it also seems somewhat tricky. this crate might help.
// don't forget RUST_LOG=debug/debug etc before cargo test -- --nocapture
#[tokio::test]
#[test_log::test]
pub async fn test_litecoin_get_address_balance() {
    let addr_1 = std::env::var("ADDR_1").unwrap();
    let confs = 1;
    let b = litecoin::get_address_balance::get_address_balance(
        ReceivingPaymentAddress(addr_1.clone()),
        confs,
    )
    .await
    .unwrap();
    let b_utxos = litecoin::get_address_balance::get_address_balance_for_small_payments(
        ReceivingPaymentAddress(addr_1.clone()),
    )
    .await
    .unwrap();
    tracing::debug!("Balance using get address: {b:?}");
    tracing::debug!("Balance using get address utxos: {b_utxos:?}");
}

#[tokio::test]
#[test_log::test]
pub async fn test_litecoin_get_recommended_fees() {
    let rec_fee =
        litecoin::get_a_fee_estimate_per_virtual_byte(litecoin::RecommendedFeeType::FastestFee)
            .await
            .unwrap();
    tracing::debug!("Recommended fee: {rec_fee:?}");
}

#[tokio::test]
#[test_log::test]
pub async fn test_litecoin_get_utxos() {
    let addr_1 = std::env::var("ADDR_1").unwrap();
    let addr_2 = std::env::var("ADDR_2").unwrap();
    let addr_3 = std::env::var("ADDR_3").unwrap();
    let fee_per_vbyte =
        litecoin::get_a_fee_estimate_per_virtual_byte(litecoin::RecommendedFeeType::FastestFee)
            .await
            .unwrap();
    let utxos = main::litecoin::get_utxos::get_utxos(&addr_1).await.unwrap();
    let tx = prepare_sweep_tx(
        utxos.clone(),
        payment_system_structs::ReceivingPaymentAddress(addr_2.to_string()),
        payment_system_structs::ReceivingPaymentAddress(addr_3.to_string()),
        fee_per_vbyte.clone(),
    )
    .await
    .unwrap();

    tracing::debug!("Recommended fee: {fee_per_vbyte:?}");
    tracing::debug!("Utxos: {utxos:?} \n Count: {}", utxos.len());
    tracing::debug!("Tx: {tx:?}");
}
