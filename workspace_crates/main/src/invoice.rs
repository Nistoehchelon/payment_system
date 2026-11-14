use std::time::Duration;

use payment_system_structs::{PaymentMethod, PaymentSqlxShape, ReceivingPaymentAddress};

use crate::get_interval;

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
    pub swept_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn get_active_invoices(
    db_pool: &sqlx::PgPool,
    max_active_invoice_time: std::time::Duration,
) -> anyhow::Result<Vec<Invoice>> {
    let interval = get_interval(max_active_invoice_time)?;
    let r = sqlx::query_as!(
        PaymentSqlxShape,
        "SELECT * FROM invoices  WHERE completed_at IS NULL
       AND created_at > NOW() - $1::interval",
        &interval
    )
    .fetch_all(db_pool)
    .await
    .unwrap_or_default();
    let r = process_result_into_vec(r)?;

    Ok(r)
}

pub struct PaidUnsweptSufficientlyOldInvoiceForSweeping(pub Invoice);
pub async fn get_paid_but_not_swept_sufficiently_old_invoices(
    db_pool: &sqlx::PgPool,
    sufficiently_old_is_at_least_x_minutes: Duration,
) -> anyhow::Result<Vec<PaidUnsweptSufficientlyOldInvoiceForSweeping>> {
    let interval = get_interval(sufficiently_old_is_at_least_x_minutes)?;
    let r = sqlx::query_as!(
        PaymentSqlxShape,
        "SELECT * FROM invoices  
        WHERE completed_at IS NOT NULL AND completed_at < NOW() - $1::interval AND swept_at IS NULL
        ORDER BY completed_at ASC",
        &interval
    )
    .fetch_all(db_pool)
    .await
    .unwrap_or_default();
    let r = process_result_into_vec(r)?
        .iter()
        .map(|i| PaidUnsweptSufficientlyOldInvoiceForSweeping(i.clone()))
        .collect();
    Ok(r)
}

fn process_result_into_vec(r: Vec<PaymentSqlxShape>) -> Result<Vec<Invoice>, anyhow::Error> {
    let r = r
        .iter()
        .map(map_into_struct)
        .collect::<anyhow::Result<Vec<Invoice>>>()?;
    Ok(r)
}

pub fn map_into_struct(i: &PaymentSqlxShape) -> Result<Invoice, anyhow::Error> {
    let mapped = Invoice {
        id: InvoiceId(i.id.clone()),
        status_update_callback_url: StatusUpdateCallbackUrl(i.status_update_callback_url.clone()),
        success_callback_url: SuccessCallbackUrl(i.success_callback_url.clone()),
        method: i.method.parse()?,
        receiving_payment_address: payment_system_structs::ReceivingPaymentAddress(
            i.receiving_payment_address.clone(),
        ),
        amount: PaymentAmount(i.amount.try_into()?),
        created_at: i.created_at,
        completed_at: i.completed_at,
        swept_at: i.swept_at,
    };
    Ok(mapped)
}
