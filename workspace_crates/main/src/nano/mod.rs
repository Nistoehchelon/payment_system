use cryptocurrencies_stuff::currencies::{CurrencyAtomicUnitRaw, XNOAtomicUnit};
use payment_system_structs::ReceivingPaymentAddress;

#[tracing::instrument(skip_all)]
pub async fn check_payment(
    api_key: &str,
    api_url: &str,
    rpc: &reqwest::Client,
    address: &ReceivingPaymentAddress,
    balance_required_to_treat_payment_as_completed: XNOAtomicUnit,
) -> anyhow::Result<bool> {
    let amounts = account_balance(rpc, api_key, api_url, &address.0)
        .await
        .map_err(|e| anyhow::anyhow!("{e:#?}"))?;
    let total = amounts.balance_raw()? + amounts.receivable_raw()?;
    tracing::debug!("Total: {total}");
    Ok(total
        >= balance_required_to_treat_payment_as_completed
            .as_atomic()
            .as_raw())
}

use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct AccountBalanceRequest<'a> {
    action: &'a str,
    account: &'a str,
}

#[derive(Deserialize, Debug)]
pub struct AccountBalanceResponse {
    /// Owned funds, in raw (string to preserve u128 precision).
    pub balance: String,
    /// Incoming sends not yet received (deprecated alias).
    pub pending: String,
    /// Incoming sends not yet received.
    pub receivable: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("node error: {0}")]
    Node(String),

    #[error("failed to parse amount: {0}")]
    ParseAmount(String),
}

pub async fn account_balance(
    client: &Client,
    api_key: &str,
    rpc_url: &str,
    account: &str,
) -> Result<AccountBalanceResponse, RpcError> {
    let body = AccountBalanceRequest {
        action: "account_balance",
        account,
    };

    // reqwest sets Content-Type: application/json automatically with .json().
    let resp = client
        .post(rpc_url)
        .header("nodes-api-key", api_key)
        .json(&body)
        .send()
        .await?;

    // Surface HTTP status and node-level errors instead of silently parsing.
    let status = resp.status();
    let text = resp.text().await?;

    if !status.is_success() {
        return Err(RpcError::Node(format!("HTTP {status}: {text}")));
    }

    // Nano nodes return {"error":"..."} with HTTP 200 for RPC-level errors.
    if let Ok(err_obj) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(err) = err_obj.get("error").and_then(|v| v.as_str())
    {
        return Err(RpcError::Node(err.to_string()));
    }

    let parsed: AccountBalanceResponse = serde_json::from_str(&text)
        .map_err(|e| RpcError::Node(format!("bad response: {e}: {text}")))?;
    Ok(parsed)
}

// Convenience: parse the raw amounts (u128, since 1 XNO = 10^30 raw).
impl AccountBalanceResponse {
    pub fn balance_raw(&self) -> Result<CurrencyAtomicUnitRaw, RpcError> {
        self.balance
            .parse::<CurrencyAtomicUnitRaw>()
            .map_err(|_| RpcError::ParseAmount(self.balance.clone()))
    }
    pub fn receivable_raw(&self) -> Result<CurrencyAtomicUnitRaw, RpcError> {
        self.receivable
            .parse::<CurrencyAtomicUnitRaw>()
            .map_err(|_| RpcError::ParseAmount(self.receivable.clone()))
    }
}
