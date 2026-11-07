use std::time::Duration;

use payment_system_structs::{Payment, PaymentId, PaymentSystemConfig};
use sqlx::postgres::types::PgInterval;
use tokio_util::sync::CancellationToken;

use crate::{fetch_callback_headers, monero::{check_payment, sweep_all}};



pub async fn poll_payments(
    db_pool: sqlx::PgPool,
    payment_system_config: PaymentSystemConfig,
    cancellation_token: CancellationToken,
    sleep_between_each_payment_check: u64,
    sleep_between_each_loop: u64,
) {
    let rpc = reqwest::Client::new();
    let interval = PgInterval {
        months: 0,
        days: 0,
        microseconds: (payment_system_config.active_invoice_minutes_since_creation as i64)
            * 60
            * 1_000_000,
    };

    loop {
        let db_pool = db_pool.clone();
        let payments = sqlx::query_as!(
            Payment,
            "SELECT * FROM payments  WHERE completed_at IS NULL
       AND created_at > NOW() - $1::interval",
            &interval
        )
        .fetch_all(&db_pool)
        .await
        .unwrap_or_default();
        println!("Polling for {} payments", payments.len());
        for payment in payments {
            match payment.method.as_str() {
                "monero" => {
                    let Ok(paid) = check_payment(
                        db_pool.clone(),
                        &payment,
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
                        let _ = sweep_all(&rpc, &payment_system_config).await;
                        let result = sqlx::query!(
                            "UPDATE payments 
                             SET completed_at = NOW() 
                             WHERE id = $1 
                             AND completed_at IS NULL", // guard against double update
                            &payment.id
                        )
                        .execute(&mut *tx)
                        .await;
                        if result.is_err() {
                            println!("Failed to mark invoice {} as complete", payment.id);
                            continue;
                        }
                        let Ok(headers) =
                            fetch_callback_headers(&mut tx, PaymentId(payment.id.clone())).await
                        else {
                            continue;
                        };

                        let mut request = rpc.post(&payment.success_callback);
                        println!(
                            "Sending success_callback {}, payment_id: {}",
                            payment.success_callback, payment.id
                        );
                        request = request.body(format!("payment_id={}", payment.id));
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
                "litecoin" => todo!(),
                _ => continue,
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

async fn cancellable_sleep(
    duration: Duration,
    cancellation_token: &CancellationToken,
) -> Result<(), ()> {
    tokio::select! {
        _ = tokio::time::sleep(duration) => Ok(()),
        _ = cancellation_token.cancelled() => Err(()),
    }
}
