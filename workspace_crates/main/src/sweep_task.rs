use payment_system_structs::PaymentSystemConfig;
use tokio_util::sync::CancellationToken;

use crate::{
    cancellable_sleep,
    invoice::{
        Invoice, PaidUnsweptSufficientlyOldInvoiceForSweeping,
        get_paid_but_not_swept_sufficiently_old_invoices,
    },
    litecoin::transfer_to_treasury_wallet_after_receiving_payment::sweep_balance,
    monero::sweep_all,
};
/*
// Now maybe sweep, but I do not need to do it in the same transaction...
   let mut sweep_tx = db_pool.begin().await?;
   sweep(invoice, &mut sweep_tx, payment_system_config.clone()).await?;
   sweep_tx.commit().await?;
    */

#[tracing::instrument]
pub async fn sweep_task(
    db_pool: sqlx::PgPool,
    payment_system_config: PaymentSystemConfig,
    cancellation_token: CancellationToken,
    sleep_between_each_payment_check: u64,
    sleep_between_each_loop: u64,
) {
    // 1. Get a list of all invoices that were completed at least 30 minutes ago and are yet to be marked as sweeped.
    // 2. Based on payment method, call the corresponding sweep function
    // 3. That's it
    let rpc = reqwest::Client::new();

    loop {
        let db_pool = db_pool.clone();
        let ready_unswept_invoices = get_paid_but_not_swept_sufficiently_old_invoices(
            &db_pool,
            payment_system_config.sufficiently_old_to_sweep_means,
        )
        .await
        .unwrap_or_default();
        tracing::debug!(
            "Will begin sweeping {} ready invoices",
            ready_unswept_invoices.len()
        );
        for invoice in ready_unswept_invoices {
            if cancellable_sleep(
                std::time::Duration::from_secs(sleep_between_each_payment_check),
                &cancellation_token,
            )
            .await
            .is_err()
            {
                tracing::debug!("Task cancelled during invoice iteration sleep");
                return;
            }
        }
        if cancellable_sleep(
            std::time::Duration::from_secs(sleep_between_each_loop),
            &cancellation_token,
        )
        .await
        .is_err()
        {
            tracing::debug!("Task cancelled during loop iteration sleep");
            return;
        }
    }
}

async fn sweep(
    invoice: PaidUnsweptSufficientlyOldInvoiceForSweeping,
    conn: &mut sqlx::PgConnection,
    payment_system_config: payment_system_structs::PaymentSystemConfig,
) -> anyhow::Result<()> {
    let rpc_client = reqwest::Client::new();
    let invoice = invoice.0;
    match invoice.method {
        payment_system_structs::PaymentMethod::Litecoin => {
            let from_index: u64 = sqlx::query!(
                "SELECT index FROM litecoin_address_indexes WHERE invoice_id = $1",
                invoice.id.0
            )
            .fetch_one(&mut *conn)
            .await?
            .index
            .try_into()?;
            sweep_balance(
                payment_system_config.litecoin_master_seed.clone(),
                from_index,
                payment_system_structs::CryptoAddress(
                    payment_system_config.litecoin_treasury_address.clone(),
                ),
            )
            .await?;
        }
        payment_system_structs::PaymentMethod::Monero => {
            sweep_all(&rpc_client, &payment_system_config).await?;
        }
    }
    sqlx::query!(
        "UPDATE invoices 
                             SET swept_at = NOW() 
                             WHERE id = $1 
                             AND swept_at IS NULL",
        &invoice.id.0
    )
    .execute(conn)
    .await?;
    Ok(())
}
