use main::litecoin;



// Apparently tracing initialization is needed in tests
// it also seems somewhat tricky. this crate might help.
// don't forget RUST_LOG=debug/info etc before cargo test -- --nocapture
#[tokio::test]
#[test_log::test]
pub async fn test_litecoin_get_address_balance() {
    let b = litecoin::get_balance("ltc1qp7cnlxmz8wgc93g0m020ckru2s55t25y3wunf6").await.unwrap();
    tracing::info!("{b:?}");
}

#[tokio::test]
#[test_log::test]
pub async fn test_litecoin_get_recommended_fees() {
    let rec_fee = litecoin::get_a_fee_estimate_per_virtual_byte(litecoin::RecommendedFeeType::FastestFee).await.unwrap();
    tracing::info!("Recommended fee: {rec_fee:?}");
}

