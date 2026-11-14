use std::time::Duration;

use payment_system_structs::PaymentSystemConfig;
use tokio_util::sync::CancellationToken;

use crate::{
    cancellable_sleep,
    invoice::{
        PaidUnsweptSufficientlyOldInvoiceForSweeping,
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

#[tracing::instrument(skip(db_pool, cancellation_token))]
pub async fn sweep_task(
    db_pool: sqlx::PgPool,
    payment_system_config: PaymentSystemConfig,
    cancellation_token: CancellationToken,
    sleep_between_each_payment_check: std::time::Duration,
    sleep_between_each_loop: std::time::Duration,
) {
    // 1. Get a list of all invoices that were completed at least 30 minutes ago and are yet to be marked as sweeped.
    // 2. Based on payment method, call the corresponding sweep function
    // 3. That's it
    let reqwest_client = reqwest::Client::new();
    let worker = SweepWorker::new(
        db_pool,
        reqwest_client,
        payment_system_config,
        cancellation_token,
    );
    let _result = worker.run_cycle().await;
}

pub struct SweepWorker {
    db_pool: sqlx::PgPool,
    reqwest_client: reqwest::Client,
    payment_system_config: PaymentSystemConfig,
    cancellation_token: CancellationToken,
    iteration_delay: Duration,
    loop_delay: Duration,
}

impl SweepWorker {
    pub fn new(
        db_pool: sqlx::PgPool,
        reqwest_client: reqwest::Client,
        payment_system_config: PaymentSystemConfig,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self {
            db_pool,
            reqwest_client,
            payment_system_config: payment_system_config.clone(),
            cancellation_token,
            iteration_delay: payment_system_config.wait_between_each_payment_check,
            loop_delay: payment_system_config.wait_between_each_payment_poll_iteration,
        }
    }
    #[tracing::instrument(skip(self))]
    pub async fn run(self) {
        while self.run_cycle().await.is_ok() {}
    }

    async fn run_cycle(&self) -> Result<(), ()> {
        let ready_unswept_invoices = get_paid_but_not_swept_sufficiently_old_invoices(
            &self.db_pool,
            self.payment_system_config.sufficiently_old_to_sweep_means,
        )
        .await
        .unwrap_or_default();

        tracing::debug!(
            "Will begin sweeping {} ready invoices",
            ready_unswept_invoices.len()
        );

        for invoice in ready_unswept_invoices {
            if let Err(e) = self.try_sweep_invoice(invoice).await {
                tracing::error!("Sweep initiation failed: {e:#?}");
            }

            if cancellable_sleep(self.iteration_delay, &self.cancellation_token)
                .await
                .is_err()
            {
                tracing::debug!("Task cancelled during invoice iteration sleep");
                return Err(());
            }
        }

        if cancellable_sleep(self.loop_delay, &self.cancellation_token)
            .await
            .is_err()
        {
            tracing::debug!("Task cancelled during loop iteration sleep");
            return Err(());
        }

        Ok(())
    }
    async fn try_sweep_invoice(
        &self,
        invoice: PaidUnsweptSufficientlyOldInvoiceForSweeping,
    ) -> anyhow::Result<()> {
        let mut sweep_tx = self.db_pool.begin().await?;
        self.sweep_inner(invoice, &mut sweep_tx).await?;
        sweep_tx.commit().await?;
        tracing::debug!("A sweep successfully initiated");
        Ok(())
    }
    async fn sweep_inner(
        &self,
        invoice: PaidUnsweptSufficientlyOldInvoiceForSweeping,
        conn: &mut sqlx::PgConnection,
    ) -> anyhow::Result<()> {
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
                    self.payment_system_config.litecoin_master_seed.clone(),
                    from_index,
                    payment_system_structs::CryptoAddress(
                        self.payment_system_config.litecoin_treasury_address.clone(),
                    ),
                )
                .await?;
            }
            payment_system_structs::PaymentMethod::Monero => {
                sweep_all(&self.reqwest_client, &self.payment_system_config).await?;
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
}
