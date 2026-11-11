use std::time::Duration;

use cryptocurrencies_stuff::currencies::LTCAtomicUnit;
use payment_system_structs::{
    CryptoAddress, PaymentId,
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
            let paid = match invoice.method {
                Monero => {
                    match monero::check_payment(
                        db_pool.clone(),
                        &invoice,
                        &rpc,
                        &payment_system_config,
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
            if paid {
                if let Err(e) = process_invoice_that_was_just_paid(
                    &payment_system_config,
                    &rpc,
                    &db_pool,
                    invoice,
                )
                .await
                {
                    tracing::error!("{e}");
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

async fn process_invoice_that_was_just_paid(
    payment_system_config: &PaymentSystemConfig,
    rpc: &reqwest::Client,
    db_pool: &sqlx::Pool<sqlx::Postgres>,
    invoice: Invoice,
) -> anyhow::Result<()> {
    let mut tx = db_pool.begin().await?;
    successful_payment(&mut tx, invoice.clone()).await?;
    send_success_webhook(rpc, invoice.clone(), &mut tx).await?;
    tx.commit().await?;
    // Now maybe sweep, but I do not need to do it in the same transaction...
    let mut sweep_tx = db_pool.begin().await?;
    let _ = sweep(invoice, &mut sweep_tx, payment_system_config.clone()).await?;
    let _ = sweep_tx.commit().await?;

    Ok(())
}

async fn successful_payment(tx: &mut sqlx::PgConnection, invoice: Invoice) -> anyhow::Result<()> {
    let _update_result = sqlx::query!(
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

async fn sweep(
    invoice: Invoice,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    payment_system_config: PaymentSystemConfig,
) -> anyhow::Result<()> {
    let rpc_client = reqwest::Client::new();
    match invoice.method {
        Litecoin => {
            litecoin::transfer_to_treasury_wallet_after_receiving_payment::sweep_balance(
                payment_system_config.litecoin_master_seed.clone(),
                0,
                CryptoAddress(payment_system_config.litecoin_treasury_address.clone()),
            )
            .await?;
        }
        Monero => {
            monero::sweep_all(&rpc_client, &payment_system_config).await?;
        }
    }
    sqlx::query!(
        "UPDATE invoices 
                             SET sweeped_at = NOW() 
                             WHERE id = $1 
                             AND sweeped_at IS NULL",
        &invoice.id.0
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
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
#[derive(Debug, Clone)]
pub struct InvoiceId(pub String);
#[derive(Debug, Clone)]
pub struct StatusUpdateCallbackUrl(pub String);
#[derive(Debug, Clone)]
pub struct SuccessCallbackUrl(pub String);
#[derive(Debug, Clone)]
pub struct PaymentAmount(pub u64);

#[derive(Debug, Clone)]
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
