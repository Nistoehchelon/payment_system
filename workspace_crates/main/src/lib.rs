use payment_system_structs::{CallbackHeaders, PaymentId};
use sqlx::{PgConnection, postgres::types::PgInterval};
pub mod dynamic_invoice_address_generation;
pub mod invoice;
pub mod litecoin;
pub mod macros_400;
pub mod monero;
pub mod poll_payments_task;
pub mod sweep_task;
async fn fetch_zero_or_more_callback_headers(
    conn: &mut PgConnection,
    payment_id: PaymentId,
) -> anyhow::Result<CallbackHeaders> {
    let rows = sqlx::query!(
        "SELECT key, value FROM callback_headers WHERE payment_id = $1",
        payment_id.to_string()
    )
    .fetch_all(conn)
    .await?;

    let map = rows.into_iter().map(|r| (r.key, r.value)).collect();
    Ok(CallbackHeaders(map))
}

pub fn get_interval(time: std::time::Duration) -> anyhow::Result<PgInterval> {
    PgInterval::try_from(time)
        .map_err(|e| anyhow::anyhow!("Failed to convert Duration into PgInterval: {e:#?}"))
}

pub struct TimeToCancel;
#[tracing::instrument]
pub async fn cancellable_sleep(
    duration: std::time::Duration,
    cancellation_token: &tokio_util::sync::CancellationToken,
) -> Result<(), TimeToCancel> {
    tracing::debug!("Sleeping for a bit in a cancellable way");
    tokio::select! {
        _ = tokio::time::sleep(duration) => Ok(()),
        _ = cancellation_token.cancelled() => Err(TimeToCancel),
    }
}
