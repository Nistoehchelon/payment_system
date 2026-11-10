use main::poll_payments::poll_payments;
use main::{ok_or_400, some_or_400};
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

    let _ = ServerBuilder::new(server_config, app_executable_version::get(), handler)
        .add_background_tasks(vec![poll_payments_task], cancellation_token)
        .run_handler_and_background_tasks()
        .await;

    Ok(())
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
        "SELECT COUNT(*) FROM payments WHERE created_at > NOW() - $1::interval",
        interval
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| {
        println!("{}", e);
        internal_server_error_500()
    })? {
        Some(c) => c,
        None => {
            println!("Could get count");
            return Err(Box::new(internal_server_error_500()));
        }
    };
    println!("Counted {count} invoices");
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

use cryptocurrencies_stuff::derive_ltc::derive_ltc_bip84_address;
use payment_system_structs::{
    ReceivingPaymentAddress, AddressIndex, Amount, NewInvoiceRequest, PaymentId, PaymentInfo, PaymentMethod,
    PaymentSystemConfig,
};
use server::http_stuff::http_response::{Charset, HttpResponseData, bad_request_400, ok};
use sqlx::{PgConnection, PgPool, Pool, Postgres, Transaction};
use strum::EnumString;

#[derive(EnumString, Debug)]
#[strum(serialize_all = "snake_case")]
pub enum APIMethod {
    NewInvoice,
}

pub enum GeneratedAddress {
    Litecoin {
        index: AddressIndex,
        address: ReceivingPaymentAddress,
    },
    Monero {
        monero_payment_id: PaymentId,
        address: ReceivingPaymentAddress,
    },
}

async fn dynamic_address_generation(
    _payment_id: String,
    conn: &mut PgConnection,
    psc: &PaymentSystemConfig,
    m: &PaymentMethod,
) -> Result<GeneratedAddress, String> {
    match m {
        PaymentMethod::Litecoin => {
            let Ok(Some(index)) = sqlx::query_scalar!(
                "SELECT COUNT(*) FROM payments WHERE method = $1",
                m.to_string()
            )
            .fetch_one(&mut *conn)
            .await
            else {
                println!("Could not get count {}", m);
                return Err("...".to_string());
            };

            let index = index as u64;
            let Ok(address) = derive_ltc_bip84_address(&psc.litecoin_master_seed, index) else {
                println!("Litecoin addr error");
                return Err("Couldn't derive litecoin".to_string());
            };
            Ok(GeneratedAddress::Litecoin {
                index: AddressIndex(index),
                address: ReceivingPaymentAddress(address),
            })
        }
        PaymentMethod::Monero => {
            let (address, monero_payment_id) =
                match get_monero_address_and_payment_id(&psc.monero_wallet_rpc_url).await {
                    Ok(a) => a,
                    Err(e) => {
                        return Err(format!("Err '{e}' from monero subaddr gen"));
                    }
                };
            Ok(GeneratedAddress::Monero {
                monero_payment_id,
                address,
            })
        }
    }
}

fn parse_path(path: &str) -> Option<(&str, &str)> {
    // strips leading '/' then splits on the next '/'
    let path = path.strip_prefix('/')?;
    let (prefix, method) = path.split_once('/')?;
    Some((prefix, method))
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

        let (prefix, method_str) = some_or_400!(
            parse_path(&request_data.path),
            format!("Could not parse {}", &request_data.path)
        );
        if prefix != payment_system_config.secret_prefix {
            println!("prefix {prefix} != {}", payment_system_config.secret_prefix);
            return server::http_stuff::http_response::bad_request_400();
        }
        let token = some_or_400!(
            request_data.headers.get("x-api-token"),
            "x-api-token header not found in request"
        )
        .to_string();
        println!("User used token {token}");
        ok_or_400!(
            validate_token(db_pool.clone(), token.clone()).await,
            format!("Could not validate token {token}")
        );
        let method = ok_or_400!(
            APIMethod::from_str(method_str),
            "Could not parse method".to_string()
        );
        println!("Method: {method:#?}");
        match method {
            APIMethod::NewInvoice => {
                let mut tx = ok_or_400!(db_pool.begin().await, String::new());
                ok_or_400!(
                    deny_if_too_many_active_invoices(&mut tx, payment_system_config.clone()).await,
                    "Too many invoices active".to_string()
                );
                let invoice_req = ok_or_400!(
                    serde_json::from_slice::<NewInvoiceRequest>(&request_data.body_bytes,),
                    format!(
                        "Could not parse json from {}",
                        String::from_utf8_lossy(&request_data.body_bytes)
                    )
                );

                println!("Invoice: {invoice_req:#?}");
                let i64_amount: i64 = ok_or_400!(invoice_req.amount_atomic.try_into());
                let payment_id = ok_or_400!(sqlx::query!("INSERT INTO payments (status_update_callback, success_callback, method, address, amount) VALUES($1, $2, $3, $4, $5::bigint) RETURNING id", invoice_req.status_update_callback,  invoice_req.success_callback,  invoice_req.method.to_string(), "to_do", i64_amount).fetch_one(&mut *tx).await, "sqlx error").id;
                for (k, v) in invoice_req.callback_headers.0.iter() {
                    ok_or_400!(sqlx::query!("INSERT INTO callback_headers (payment_id, key, value) VALUES ($1, $2, $3)", payment_id.clone(), k, v, ).execute(&mut *tx).await);
                }
                let gened_address = ok_or_400!(
                    dynamic_address_generation(
                        payment_id.clone(),
                        &mut tx,
                        &payment_system_config.clone(),
                        &invoice_req.method.clone()
                    )
                    .await,
                    format!("Could not generate address for {}", invoice_req.method)
                );
                let address = match gened_address {
                    GeneratedAddress::Litecoin { index, address } => {
                        ok_or_400!(sqlx::query!("INSERT INTO litecoin_address_indexes (payment_id, index) VALUES ($1, $2)", payment_id, index.0 as i64).execute(&mut *tx).await.map_err(|e| e.to_string()));
                        address
                    }
                    GeneratedAddress::Monero {
                        monero_payment_id,
                        address,
                    } => {
                        ok_or_400!(sqlx::query!("INSERT INTO monero_integrated_payment_ids (payment_id, monero_payment_id) VALUES ($1, $2)", payment_id, monero_payment_id.to_string()).execute(&mut *tx).await.map_err(|e| e.to_string()));
                        address
                    }
                };
                ok_or_400!(
                    sqlx::query!(
                        "UPDATE payments SET address = $1 WHERE id = $2",
                        address.0,
                        payment_id
                    )
                    .execute(&mut *tx)
                    .await
                );

                ok_or_400!(tx.commit().await);
                let payment_info = PaymentInfo {
                    payment_id: PaymentId(payment_id),
                    amount: Amount(invoice_req.amount_atomic),
                    address,
                    method: invoice_req.method,
                };
                println!("{payment_info:#?}");
                let content = ok_or_400!(serde_json::to_vec(&payment_info));
                ok(content, ContentType::ApplicationJson, Some(Charset::Utf8))
            }
        }
    }

    pub fn into_handler_fn(self) -> HandlerFn {
        HandlerFn::new(move |request_data, server_config| {
            let this = self.clone();
            async move { this.handle(request_data, server_config).await }
        })
    }
}
