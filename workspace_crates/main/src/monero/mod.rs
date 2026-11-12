pub mod check_payment;
pub use check_payment::check_payment;
use payment_system_structs::{PaymentId, PaymentSystemConfig, ReceivingPaymentAddress};
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

    tracing::debug!("Called sweep_all()... response is: {res:#?}");
    anyhow::Ok(())
}

pub async fn get_monero_address_and_payment_id(
    rpc_url: &str,
) -> Result<(ReceivingPaymentAddress, PaymentId), String> {
    let client = reqwest::Client::new();

    let body = serde_json::json!({"jsonrpc": "2.0",
      "id": "0",
      "method": "make_integrated_address"
    });

    let resp = client
        .post(rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Monero RPC request failed: {e}"))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Monero RPC parse failed: {e}"))?;

    let address = resp["result"]["integrated_address"]
        .as_str()
        .ok_or_else(|| format!("Unexpected RPC response: {resp}"))?
        .to_string();
    let payment_id = resp["result"]["payment_id"]
        .as_str()
        .ok_or_else(|| format!("Unexpected RPC response: {resp}"))?
        .to_string();

    Ok((ReceivingPaymentAddress(address), PaymentId(payment_id)))
}
