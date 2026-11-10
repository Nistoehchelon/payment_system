use std::time::Duration;

use cryptocurrencies_stuff::currencies::LTCAtomicUnit;
use payment_system_structs::{
    PaymentId,
    PaymentMethod::{self, Litecoin, Monero},
    PaymentSqlxShape, PaymentSystemConfig, ReceivingPaymentAddress,
};
use sqlx::postgres::types::PgInterval;
use tokio_util::sync::CancellationToken;

use crate::{
    fetch_zero_or_more_callback_headers, litecoin,
    monero::{self},
};

pub async fn poll_payments(
    db_pool: sqlx::PgPool,
    payment_system_config: PaymentSystemConfig,
    cancellation_token: CancellationToken,
    sleep_between_each_payment_check: u64,
    sleep_between_each_loop: u64,
) {
    let rpc = reqwest::Client::new();

    loop {
        let db_pool = db_pool.clone();
        let invoices = get_active_invoices(
            &db_pool,
            payment_system_config.active_invoice_minutes_since_creation,
        )
        .await
        .unwrap_or_default();
        println!("Polling for {} payments", invoices.len());
        for invoice in invoices {
            match invoice.method {
                Monero => {
                    let Ok(paid) = monero::check_payment(
                        db_pool.clone(),
                        &invoice,
                        &rpc,
                        &payment_system_config,
                    )
                    .await
                    else {
                        continue;
                    };
                    let Ok(mut tx) = db_pool.begin().await else {
                        continue;
                    };
                    if paid || payment_system_config.confirm_all_for_testing_purposes {
                        let _ = monero::sweep_all(&rpc, &payment_system_config).await;
                        let result = sqlx::query!(
                            "UPDATE invoices 
                             SET completed_at = NOW() 
                             WHERE id = $1 
                             AND completed_at IS NULL",
                            &invoice.id.0
                        )
                        .execute(&mut *tx)
                        .await;
                        if result.is_err() {
                            println!("Failed to mark invoice {} as complete", invoice.id.0);
                            continue;
                        }
                        let Ok(headers) =
                            fetch_zero_or_more_callback_headers(&mut tx, PaymentId(invoice.id.0.clone())).await
                        else {
                            tracing::debug!("Error fetching callback headers");
                            continue;
                        };

                        let mut request = rpc.post(invoice.success_callback_url.0.clone());
                        println!(
                            "Sending success_callback {}, payment_id: {}",
                            invoice.success_callback_url.0.clone(),
                            invoice.id.0.clone()
                        );
                        request = request.body(format!("payment_id={}", invoice.id.0.clone()));
                        for (key, value) in &headers.0 {
                            request = request.header(key, value);
                        }

                        let resp = match request.send().await {
                            Ok(resp) => resp,
                            Err(e) => {
                                println!("Callback returned error: {e:#?}");
                                continue;
                            }
                        };
                        println!(
                            "Received resp after successful payment request: {:#?}",
                            resp
                        );
                        let _ = tx.commit().await;
                    }
                }
                Litecoin => {
                    let received= match litecoin::check_payment(
                        &invoice.receiving_payment_address.clone(),
                        LTCAtomicUnit::from_atomic(invoice.amount.0),
                    )
                    .await {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::error!("{e}");
                            continue
                        }
                    };
                    match received {
                        false => continue,
                        true => {
                            let Ok(mut tx) = db_pool.begin().await else {
                                continue;
                            };
                            // mark as complete
                            let _result = sqlx::query!(
                                "UPDATE invoices 
                             SET completed_at = NOW() 
                             WHERE id = $1 
                             AND completed_at IS NULL",
                                &invoice.id.0
                            )
                            .execute(&mut *tx)
                            .await;
                            let Ok(headers) =
                                fetch_zero_or_more_callback_headers(&mut tx, PaymentId(invoice.id.0.clone()))
                                    .await
                            else {
                                continue;
                            };

                            let mut request = rpc.post(&invoice.success_callback_url.0);
                            tracing::debug!(
                                "Sending success_callback {}, payment_id: {}",
                                invoice.success_callback_url.0,
                                invoice.id.0
                            );
                            request = request.body(format!("payment_id={}", invoice.id.0));
                            for (key, value) in &headers.0 {
                                request = request.header(key, value);
                            }

                            let resp = match request.send().await {
                                Ok(resp) => resp,
                                Err(e) => {
                                    println!("Callback returned error: {e:#?}");
                                    continue;
                                }
                            };
                            tracing::debug!(
                                "Received resp after successful payment request: {:#?}",
                                resp
                            );

                            let _index: u64 = match sqlx::query!(
                                "SELECT index FROM litecoin_address_indexes WHERE payment_id = $1",
                                &invoice.id.0
                            )
                            .fetch_one(&mut *tx)
                            .await
                            {
                                Ok(i) => match i.index.try_into() {
                                    Ok(i) => i,
                                    Err(e) => {
                                        tracing::debug!("Err {e}");
                                        continue;
                                    }
                                },
                                Err(e) => {
                                    tracing::debug!("{e:?}");
                                    continue;
                                }
                            };
                            // let _ = litecoin::sweep_balance(
                            //     db_pool.clone(),
                            //     payment_system_config.litecoin_master_seed.clone(),
                            //     index as u64,
                            //     ReceivingPaymentAddress(
                            //         payment_system_config.litecoin_treasury_address.clone(),
                            //     ),
                            // )
                            // .await;
                            let _ = tx.commit().await;
                            // That's it, right?
                        }
                    }
                }
            }
            if cancellable_sleep(
                Duration::from_secs(sleep_between_each_payment_check),
                &cancellation_token,
            )
            .await
            .is_err()
            {
                tracing::info!("Poll payments task cancelled during invoice delay");
                return;
            }
        }
        if cancellable_sleep(
            Duration::from_secs(sleep_between_each_loop),
            &cancellation_token,
        )
        .await
        .is_err()
        {
            tracing::info!("Poll payments task cancelled during main sleep");
            return;
        }
    }
}

#[tracing::instrument]
async fn cancellable_sleep(
    duration: Duration,
    cancellation_token: &CancellationToken,
) -> Result<(), ()> {
    tracing::debug!("Enter");
    tokio::select! {
        _ = tokio::time::sleep(duration) => Ok(()),
        _ = cancellation_token.cancelled() => Err(()),
    }
}

pub struct InvoiceId(pub String);
pub struct StatusUpdateCallbackUrl(pub String);
pub struct SuccessCallbackUrl(pub String);
pub struct PaymentAmount(pub u64);
pub struct Invoice {
    pub id: InvoiceId,
    pub status_update_callback_url: StatusUpdateCallbackUrl,
    pub success_callback_url: SuccessCallbackUrl,
    pub method: PaymentMethod,
    pub receiving_payment_address: ReceivingPaymentAddress,
    pub amount: PaymentAmount,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub sweeped_at: Option<chrono::DateTime<chrono::Utc>>,
}
pub async fn get_active_invoices(
    db_pool: &sqlx::PgPool,
    max_active_invoice_time: u64,
) -> anyhow::Result<Vec<Invoice>> {
    let interval = PgInterval {
        months: 0,
        days: 0,
        microseconds: (max_active_invoice_time as i64) * 60 * 1_000_000,
    };
    let r = sqlx::query_as!(
        PaymentSqlxShape,
        "SELECT * FROM invoices  WHERE completed_at IS NULL
       AND created_at > NOW() - $1::interval",
        &interval
    )
    .fetch_all(db_pool)
    .await
    .unwrap_or_default();

    let r = r
        .iter()
        .map(|i| {
            let mapped = Invoice {
                id: InvoiceId(i.id.clone()),
                status_update_callback_url: StatusUpdateCallbackUrl(
                    i.status_update_callback_url.clone(),
                ),
                success_callback_url: SuccessCallbackUrl(i.success_callback_url.clone()),
                method: i.method.parse()?,
                receiving_payment_address: payment_system_structs::ReceivingPaymentAddress(
                    i.receiving_payment_address.clone(),
                ),
                amount: PaymentAmount(i.amount.try_into()?),
                created_at: i.created_at,
                completed_at: i.completed_at,
                sweeped_at: i.sweeped_at,
            };
            Ok(mapped)
        })
        .collect::<anyhow::Result<Vec<Invoice>>>()?;

    Ok(r)
}
