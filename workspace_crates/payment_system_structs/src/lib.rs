use std::collections::HashMap;

use strum::EnumString;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PaymentSystemConfig {
    pub ip: String,
    pub port: u64,
    pub rust_log_directory_path: String,
    pub wait_between_each_payment_check_secs: u64,
    pub wait_between_each_payment_poll_iteration_secs: u64,
    pub database_url: String,
    pub secret_prefix: String,
    pub monero_wallet_rpc_url: String,
    pub monero_treasury_address: String,
    pub monero_confirmations: u64,
    // limit monitoring load
    pub max_active_invoices: u64,
    pub active_invoice_minutes_since_creation: u64,
    pub master_seed: String,
    pub cf_bypass_token_header_name: String,
    pub cf_bypass_token_header_value: String,
    pub confirm_all_for_testing_purposes: bool,
}

pub struct Payment {
    pub primary_key: i64,
    pub id: String,
    pub status_update_callback: String,
    pub success_callback: String,
    pub method: String,
    pub address: String,
    pub amount: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub sweeped_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, EnumString, strum::Display, Debug)]
#[strum(ascii_case_insensitive)]
pub enum PaymentMethod {
    #[strum(serialize = "litecoin", serialize = "ltc")]
    Litecoin,
    #[strum(serialize = "monero", serialize = "xmr")]
    Monero,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct NewInvoiceRequest {
    pub invoice_id: String,
    pub method: PaymentMethod,
    pub amount_atomic: u64,
    pub success_callback: String,
    pub status_update_callback: String,
    pub callback_headers: CallbackHeaders,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CallbackHeaders(pub HashMap<String, String>);

/// Sent in response to invoice creation
#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct PaymentInfo {
    pub payment_id: PaymentId,
    pub amount: Amount,
    pub address: Address,
    pub method: PaymentMethod,
}

#[derive(
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    derive_more::Display,
    derive_more::From,
    derive_more::Into,
)]
pub struct PaymentId(pub String);

#[derive(
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    derive_more::Display,
    derive_more::From,
    derive_more::Into,
)]
pub struct Address(pub String);
#[derive(
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    derive_more::Display,
    derive_more::From,
    derive_more::Into,
)]
pub struct Amount(pub u64);
#[derive(
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    derive_more::Display,
    derive_more::From,
    derive_more::Into,
)]
pub struct AddressIndex(pub u64);
