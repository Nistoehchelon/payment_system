use payment_system_structs::{Payment, PaymentSystemConfig};
use reqwest::Client;

pub async fn sweep_all(
    rpc_client: &Client,
    payment_system_config: &PaymentSystemConfig,
) -> anyhow::Result<()> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "0",
        "method": "sweep_all",
        "params": {
            "address": &payment_system_config.monero_treasury_address,
            "account_index": 0,
            "subaddr_indices": [0],  // address index 0
            "priority": 0,
            "ring_size": 16,
            "get_tx_keys": false,
            "do_not_relay": false
        }
    });
    let res: serde_json::Value = rpc_client
        .post(&payment_system_config.monero_wallet_rpc_url)
        .json(&body)
        .send()
        .await?
        .json()
        .await?;

    println!("Called sweep_all()... response is: {res:#?}");
    anyhow::Ok(())
}



pub async fn check_payment(
    db_pool: sqlx::PgPool,
    payment: &Payment,
    client: &reqwest::Client,
    payment_system_config: &PaymentSystemConfig,
) -> anyhow::Result<bool> {
    let monero_payment_id = sqlx::query!(
        "SELECT monero_payment_id FROM monero_integrated_payment_ids WHERE payment_id = $1",
        payment.id
    )
    .fetch_one(&db_pool)
    .await?
    .monero_payment_id;
    let expected_amount: u64 = payment.amount.try_into()?; // already in piconer
    println!(
        "[Payment ID {}] Checking if the expect amount of {expected_amount} piconero was paid to {}",
        payment.id, payment.address
    );
    let body = serde_json::json!({
      "jsonrpc": "2.0",
      "id": "0",
      "method": "get_payments",
      "params": {
        "payment_id": monero_payment_id
      }
    });
    let res: serde_json::Value = client
        .post(&payment_system_config.monero_wallet_rpc_url)
        .json(&body)
        .send()
        .await?
        .json()
        .await?;

    let transfers = res["result"]["payments"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let current_height_body = serde_json::json!({"jsonrpc":"2.0","id":"0","method":"get_height"});
    let res_height: serde_json::Value = client
        .post(&payment_system_config.monero_wallet_rpc_url)
        .json(&current_height_body)
        .send()
        .await?
        .json()
        .await?;
    const MONERO_FULL_CONFIRMATIONS: u64 = 10;
    let current_blockchain_height = res_height["result"]["height"].as_u64().unwrap_or(0);
    println!("Curr height: {current_blockchain_height}");
    let mut unlocked_transaction_ids = Vec::new();
    for transfer in transfers.clone() {
        let locked = transfer["locked"].as_bool().unwrap_or(true);
        if !locked {
            let tx_hash = transfer["tx_hash"]
                .as_str()
                .unwrap_or("erm_nothing?")
                .to_string();
            if tx_hash != "erm_nothing?" {
                unlocked_transaction_ids.push(tx_hash);
            }
        }
        let transaction_height = transfer["block_height"].as_u64().unwrap_or(999999999);
        println!("transfer height: {transaction_height}");
        let transaction_confirmation_at_block = transaction_height + MONERO_FULL_CONFIRMATIONS;
        if transaction_confirmation_at_block > current_blockchain_height {
            let blocks_to_unlock = transaction_confirmation_at_block - current_blockchain_height;
            println!("A transfer received, blocks to unlock : {blocks_to_unlock}",);
        } else {
            println!("An unlocked tx");
        }
    }
    if !transfers.is_empty() {
        println!("Received some transfers!! {transfers:#?}");
    }

    let confirmed_received: u64 = transfers
        .iter()
        .filter(|t| !t["locked"].as_bool().unwrap_or(true))
        .map(|t| t["amount"].as_u64().unwrap_or(0))
        .sum();
    let total_received: u64 = transfers
        .iter()
        .map(|t| t["amount"].as_u64().unwrap_or(0))
        .sum();
    println!("Confirmed: {confirmed_received}");
    println!("Confirmed + unconfirmed: {total_received}");
    let complete = confirmed_received >= expected_amount;

    match complete {
        false => println!("not enough confirmed"),
        true => println!("yay, enough confirmed"),
    }
    Ok(confirmed_received >= expected_amount)
}
