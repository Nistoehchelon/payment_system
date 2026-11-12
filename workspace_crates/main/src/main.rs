use payment_system_structs::{
    Amount, NewInvoiceRequest, PaymentId, PaymentInfo, PaymentSystemConfig,
};
use server::http_stuff::http_response::{Charset, HttpResponseData, bad_request_400, ok};
use sqlx::{PgPool, Pool, Postgres, Transaction};
use strum::EnumString;

use main::dynamic_invoice_address_generation::{GeneratedAddress, dynamic_address_generation};
use main::ok_or_400;
use main::poll_payments::poll_payments;
use server::http_stuff::http_request_parsing::HttpRequestData;
use server::http_stuff::http_response::{ContentType, internal_server_error_500};
use server::server_config::ServerConfig;
use server::{HandlerFn, ServerBuilder};
use sqlx::postgres::types::PgInterval;
use std::{path::PathBuf, str::FromStr};
use tokio_util::sync::CancellationToken;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    let cancellation_token = CancellationToken::new();
    let poll_payments_token = cancellation_token.child_token();

    dotenvy::dotenv().ok();

    if server::will_show_version_then_would_like_to_terminate_the_program(
        app_executable_version::VERSION,
    ) {
        return Ok(());
    }

    let payment_system_config =
        server::config_reader::read_config::<PaymentSystemConfig>(PathBuf::from(".env"));
    let db_pool = sqlx::PgPool::connect(&payment_system_config.database_url)
        .await
        .expect("I expected pool construction to succeed :)");

    let db_pool_clone_for_handler = db_pool.clone();
    let payment_system_config_clone_for_handler = payment_system_config.clone();

    let handler = Router {
        db_pool: db_pool_clone_for_handler,
        payment_system_config: payment_system_config_clone_for_handler,
    }
    .into_handler_fn();

    let poll_payments_task = Box::pin(poll_payments(
        db_pool.clone(),
        payment_system_config.clone(),
        poll_payments_token,
        payment_system_config
            .clone()
            .wait_between_each_payment_check_secs,
        payment_system_config
            .clone()
            .wait_between_each_payment_poll_iteration_secs,
    ));
    let server_config = ServerConfig::new(
        payment_system_config.ip.clone(),
        payment_system_config.port,
        payment_system_config.rust_log_directory_path,
    );

    let _result = ServerBuilder::new(server_config, app_executable_version::get(), handler)
        .add_background_tasks(vec![poll_payments_task], cancellation_token)
        .run_handler_and_background_tasks()
        .await;

    Ok(())
}

#[derive(EnumString, Debug)]
#[strum(serialize_all = "snake_case")]
pub enum APIMethod {
    NewInvoice,
    CheckInvoicePaymentStatus,
}

#[derive(Clone)]
pub struct Router {
    db_pool: Pool<Postgres>,
    payment_system_config: PaymentSystemConfig,
}

impl Router {
    pub async fn handle(
        &self,
        request_data: HttpRequestData,
        _server_config: ServerConfig,
    ) -> HttpResponseData {
        let db_pool = self.db_pool.clone();
        let payment_system_config = self.payment_system_config.clone();

        let data =
            match initial_api_access_check(&request_data, &db_pool, &payment_system_config).await {
                Ok(value) => value,
                Err(value) => return *value,
            };
        process_method(data).await
    }

    pub fn into_handler_fn(self) -> HandlerFn {
        HandlerFn::new(move |request_data, server_config| {
            let this = self.clone();
            async move { this.handle(request_data, server_config).await }
        })
    }
}
pub struct ProceedWithAPIRequest {
    pub body_bytes: Vec<u8>,
    pub db_pool: Pool<Postgres>,
    pub payment_system_config: PaymentSystemConfig,
    pub method: APIMethod,
}

pub async fn initial_api_access_check(
    request_data: &HttpRequestData,
    db_pool: &Pool<Postgres>,
    payment_system_config: &PaymentSystemConfig,
) -> anyhow::Result<ProceedWithAPIRequest, Box<HttpResponseData>> {
    let Some((prefix, method_str)) = parse_path(&request_data.path) else {
        tracing::error!("Error parsing {}", request_data.path);
        return Err(Box::new(
            server::http_stuff::http_response::bad_request_400(),
        ));
    };
    if prefix != payment_system_config.secret_prefix {
        tracing::error!("prefix {prefix} != {}", payment_system_config.secret_prefix);
        return Err(Box::new(
            server::http_stuff::http_response::bad_request_400(),
        ));
    }
    let Some(token) = request_data.headers.get("x-api-token") else {
        tracing::error!("x-api-token header not found in request");
        return Err(Box::new(bad_request_400()));
    };
    let token = token.to_string();
    tracing::debug!("User used token {token}");
    if let Err(e) = validate_token(db_pool.clone(), token.clone()).await {
        tracing::error!("TokenValidationError: {e}");
        return Err(Box::new(bad_request_400()));
    }
    let Ok(method) = APIMethod::from_str(method_str) else {
        tracing::error!("Could not parse method {method_str}");
        return Err(Box::new(bad_request_400()));
    };
    Ok(ProceedWithAPIRequest {
        body_bytes: request_data.body_bytes.clone(),
        db_pool: db_pool.clone(),
        payment_system_config: payment_system_config.clone(),
        method,
    })
}

pub async fn process_method(data: ProceedWithAPIRequest) -> HttpResponseData {
    tracing::debug!("Method: {:#?}", data.method);
    match data.method {
        APIMethod::NewInvoice => {
            process_new_invoice_method(&data.body_bytes, data.db_pool, data.payment_system_config)
                .await
        }
        APIMethod::CheckInvoicePaymentStatus => {
            process_check_payment_status_method(
                &data.body_bytes,
                data.db_pool,
                data.payment_system_config,
            )
            .await
        }
    }
}
pub async fn process_check_payment_status_method(
    _body_bytes: &[u8],
    _db_pool: Pool<Postgres>,
    _payment_system_config: PaymentSystemConfig,
) -> HttpResponseData {
    todo!()
}
pub async fn process_new_invoice_method(
    body_bytes: &[u8],
    db_pool: Pool<Postgres>,
    payment_system_config: PaymentSystemConfig,
) -> HttpResponseData {
    let mut tx = ok_or_400!(db_pool.begin().await, String::new());
    ok_or_400!(
        deny_if_too_many_active_invoices(&mut tx, payment_system_config.clone()).await,
        "Too many invoices active".to_string()
    );
    let invoice_req = ok_or_400!(
        serde_json::from_slice::<NewInvoiceRequest>(body_bytes),
        format!(
            "Could not parse json from {}",
            String::from_utf8_lossy(body_bytes)
        )
    );

    tracing::debug!("Invoice: {invoice_req:#?}");
    let i64_amount: i64 = ok_or_400!(invoice_req.amount_atomic.try_into());
    let invoice_id = ok_or_400!(sqlx::query!("INSERT INTO invoices (status_update_callback_url, success_callback_url, method, receiving_payment_address, amount) VALUES($1, $2, $3, $4, $5::bigint) RETURNING id", invoice_req.status_update_callback_url,  invoice_req.success_callback_url,  invoice_req.method.to_string(), "to_do", i64_amount).fetch_one(&mut *tx).await, "sqlx error").id;

    for (k, v) in invoice_req.callback_headers.0.iter() {
        ok_or_400!(
            sqlx::query!(
                "INSERT INTO callback_headers (payment_id, key, value) VALUES ($1, $2, $3)",
                invoice_id.clone(),
                k,
                v,
            )
            .execute(&mut *tx)
            .await
        );
    }
    let gened_address = ok_or_400!(
        dynamic_address_generation(
            invoice_id.clone(),
            &mut tx,
            &payment_system_config.clone(),
            &invoice_req.method.clone()
        )
        .await,
        format!("Could not generate address for {}", invoice_req.method)
    );
    let address = match gened_address {
        GeneratedAddress::Litecoin { index, address } => {
            ok_or_400!(
                sqlx::query!(
                    "INSERT INTO litecoin_address_indexes (invoice_id, index) VALUES ($1, $2)",
                    invoice_id,
                    index.0 as i64
                )
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())
            );
            address
        }
        GeneratedAddress::Monero {
            monero_payment_id,
            address,
        } => {
            ok_or_400!(sqlx::query!("INSERT INTO monero_integrated_payment_ids (invoice_id, monero_payment_id) VALUES ($1, $2)", invoice_id, monero_payment_id.to_string()).execute(&mut *tx).await.map_err(|e| e.to_string()));
            address
        }
    };
    ok_or_400!(
        sqlx::query!(
            "UPDATE invoices SET receiving_payment_address = $1 WHERE id = $2",
            address.0,
            invoice_id
        )
        .execute(&mut *tx)
        .await
    );

    ok_or_400!(tx.commit().await);
    let payment_info = PaymentInfo {
        payment_id: PaymentId(invoice_id),
        amount: Amount(invoice_req.amount_atomic),
        address,
        method: invoice_req.method,
    };
    tracing::debug!("{payment_info:#?}");
    let content = ok_or_400!(serde_json::to_vec(&payment_info));
    ok(content, ContentType::ApplicationJson, Some(Charset::Utf8))
}

async fn deny_if_too_many_active_invoices(
    tx: &mut Transaction<'_, Postgres>,
    payment_system_config: PaymentSystemConfig,
) -> Result<(), Box<HttpResponseData>> {
    let interval = PgInterval {
        months: 0,
        days: 0,
        microseconds: (payment_system_config.active_invoice_minutes_since_creation as i64)
            * 60
            * 1_000_000,
    };
    let count = match sqlx::query_scalar!(
        "SELECT COUNT(*) FROM invoices WHERE created_at > NOW() - $1::interval",
        interval
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| {
        tracing::error!("{}", e);
        internal_server_error_500()
    })? {
        Some(c) => c,
        None => {
            tracing::error!("Could get count");
            return Err(Box::new(internal_server_error_500()));
        }
    };
    tracing::debug!("Counted {count} invoices");
    let count = match u64::try_from(count) {
        Ok(c) => c,
        Err(err) => {
            tracing::debug!("{err}");
            return Err(Box::new(bad_request_400()));
        }
    };
    if count > payment_system_config.max_active_invoices {
        return Err(Box::new(bad_request_400()));
    }
    Ok(())
}

#[derive(Debug, strum::Display)]
pub enum TokenValidationError {
    NoTokenInHeaders,
    DbError,
    TokenNotFound,
}
async fn validate_token(db_pool: PgPool, token_str: String) -> Result<(), TokenValidationError> {
    let _token = {
        let exists: bool =
            match sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tokens WHERE token = $1)")
                .bind(&token_str)
                .fetch_one(&db_pool)
                .await
            {
                Ok(b) => b,
                Err(_) => {
                    return Err(TokenValidationError::DbError);
                }
            };
        if !exists {
            return Err(TokenValidationError::TokenNotFound);
        }
        token_str.to_string()
    };
    Ok(())
}

#[tracing::instrument]
fn parse_path(path: &str) -> Option<(&str, &str)> {
    // strips leading '/' then splits on the next '/'
    tracing::debug!("Parsing path {path}");
    let path = path.strip_prefix('/')?;
    let (prefix, method) = path.split_once('/')?;
    Some((prefix, method))
}
