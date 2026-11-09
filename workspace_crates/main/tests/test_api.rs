use main::litecoin;

#[tokio::test]
pub async fn test_litecoin_get_address_balance() {
    let b = litecoin::get_balance("ltc1qp7cnlxmz8wgc93g0m020ckru2s55t25y3wunf6").await.unwrap();
    tracing::debug!("{b:?}");
    println!("{b:?}");
}