use main::nano::account_balance;

#[tokio::test]
#[test_log::test]
pub async fn nano_get_balance() {
    let addr = std::env::var("ADDR").unwrap();
    let api_url = std::env::var("API_URL").unwrap();
    let api_key = std::env::var("API_KEY").unwrap();
    tracing::debug!(
        "{:#?}",
        account_balance(&reqwest::Client::new(), &api_key, &api_url, &addr)
            .await
            .unwrap()
    );
}
