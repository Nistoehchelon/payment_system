use payment_system_structs::{CallbackHeaders, PaymentId};
use sqlx::PgConnection;
pub mod litecoin;
pub mod macros_400;
pub mod monero;
pub mod poll_payments;

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
