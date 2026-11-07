use payment_system_structs::{CallbackHeaders, PaymentId};
use sqlx::PgConnection;
pub mod macros_400;
pub mod poll_payments;
pub mod litecoin;
pub mod monero;

async fn fetch_callback_headers(
    conn: &mut PgConnection,
    payment_id: PaymentId,
) -> Result<CallbackHeaders, String> {
    let Ok(rows) = sqlx::query!(
        "SELECT key, value FROM callback_headers WHERE payment_id = $1",
        payment_id.to_string()
    )
    .fetch_all(conn)
    .await
    else {
        return Err("DB err".to_string());
    };

    let map = rows.into_iter().map(|r| (r.key, r.value)).collect();
    Ok(CallbackHeaders(map))
}
