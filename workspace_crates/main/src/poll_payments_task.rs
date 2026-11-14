use std::time::Duration;

use cryptocurrencies_stuff::currencies::LTCAtomicUnit;
use payment_system_structs::{
    PaymentId,
    PaymentMethod::{Litecoin, Monero},
    PaymentSystemConfig, 
};
use tokio_util::sync::CancellationToken;

use crate::{
    cancellable_sleep, fetch_zero_or_more_callback_headers, invoice::{Invoice, get_active_invoices}, litecoin, monero::{self},
};

#[tracing::instrument]
pub async fn poll_payments_tasks(
    db_pool: sqlx::PgPool,
    payment_system_config: PaymentSystemConfig,
    cancellation_token: CancellationToken,
    sleep_between_each_payment_check: Duration,
    sleep_between_each_loop: Duration,
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
        tracing::debug!("Polling for {} payments", invoices.len());
        for invoice in invoices {
            let paid = match invoice.method {
                Monero => {
                    match monero::check_payment(
                        db_pool.clone(),
                        &invoice,
                        &rpc,
                        &payment_system_config.monero_wallet_rpc_url,
                        payment_system_config.monero_confirmations,
                    )
                    .await
                    {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::error!("{e}");
                            continue;
                        }
                    }
                }
                Litecoin => {
                    match litecoin::check_payment(
                        &invoice.receiving_payment_address.clone(),
                        LTCAtomicUnit::from_atomic(invoice.amount.0),
                    )
                    .await
                    {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::error!("{e}");
                            continue;
                        }
                    }
                }
            };
            if paid
                && let Err(e) = process_invoice_that_was_just_paid(
                    &rpc,
                    &db_pool,
                    invoice,
                )
                .await
                {
                    tracing::error!("{e}");
                }
            if cancellable_sleep(
                sleep_between_each_payment_check,
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
            sleep_between_each_loop,
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

async fn process_invoice_that_was_just_paid(
    rpc: &reqwest::Client,
    db_pool: &sqlx::Pool<sqlx::Postgres>,
    invoice: Invoice,
) -> anyhow::Result<()> {
    let mut tx = db_pool.begin().await?;
    successful_payment(&mut tx, invoice.clone()).await?;
    send_success_webhook(rpc, invoice.clone(), &mut tx).await?;
    tx.commit().await?;
    Ok(())
}

async fn successful_payment(tx: &mut sqlx::PgConnection, invoice: Invoice) -> anyhow::Result<()> {
    sqlx::query!(
        "UPDATE invoices 
                             SET completed_at = NOW() 
                             WHERE id = $1 
                             AND completed_at IS NULL",
        &invoice.id.0
    )
    .execute(&mut *tx)
    .await?;

    Ok(())
}

async fn send_success_webhook(
    rpc: &reqwest::Client,
    invoice: Invoice,
    tx: &mut sqlx::PgConnection,
) -> anyhow::Result<()> {
    let headers =
        fetch_zero_or_more_callback_headers(&mut *tx, PaymentId(invoice.id.0.clone())).await?;
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
    let resp = request.send().await?;
    tracing::debug!(
        "Received resp after successful payment request: {:#?}",
        resp
    );
    Ok(())
}
