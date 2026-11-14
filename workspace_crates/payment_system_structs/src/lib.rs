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
     #[serde(with = "humantime_serde")]
    pub active_invoice_minutes_since_creation: std::time::Duration,
     #[serde(with = "humantime_serde")]
    pub sufficiently_old_to_sweep_means: std::time::Duration,

    pub litecoin_master_seed: String,
    pub litecoin_rest_api_url: String,
    pub litecoin_treasury_address: String,
    pub cf_bypass_token_header_name: String,
    pub cf_bypass_token_header_value: String,
    pub confirm_all_for_testing_purposes: bool,
}

pub struct PaymentSqlxShape {
    pub primary_key: i64,
    pub id: String,
    pub status_update_callback_url: String,
    pub success_callback_url: String,
    pub method: String,
    pub receiving_payment_address: String,
    pub amount: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub swept_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, EnumString, strum::Display, Debug)]
#[strum(ascii_case_insensitive)]
#[derive(sqlx::Type)]
#[sqlx(type_name = "text")]
pub enum PaymentMethod {
    #[strum(serialize = "litecoin", serialize = "ltc")]
    Litecoin,
    #[strum(serialize = "monero", serialize = "xmr")]
    Monero,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct NewInvoiceRequest {
    pub method: PaymentMethod,
    pub amount_atomic: u64,
    pub success_callback_url: String,
    pub status_update_callback_url: String,
    pub callback_headers: CallbackHeaders,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CallbackHeaders(pub HashMap<String, String>);

/// Sent in response to invoice creation
#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct PaymentInfo {
    pub payment_id: PaymentId,
    pub amount: Amount,
    pub address: ReceivingPaymentAddress,
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
pub struct CryptoAddress(pub String);
#[derive(
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    derive_more::Display,
    derive_more::From,
    derive_more::Into,
)]
pub struct ReceivingPaymentAddress(pub String);
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
