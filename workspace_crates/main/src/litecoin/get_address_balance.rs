use anyhow::Context;
use cryptocurrencies_stuff::currencies::LTCAtomicUnit;
use payment_system_structs::ReceivingPaymentAddress;

use crate::litecoin::get_utxos::Utxo;

pub async fn get_address_balance(
    address: ReceivingPaymentAddress,
    min_confirmations: u64,
) -> anyhow::Result<LTCAtomicUnit> {
    let client = reqwest::Client::new();
    let tip_height: u64 = client
        .get("https://litecoinspace.org/api/blocks/tip/height")
        .send()
        .await
        .context("failed to reach Litecoin Space (tip height)")?
        .error_for_status()
        .context("tip height API error")?
        .text()
        .await
        .context("failed to read tip height response")?
        .trim()
        .parse()
        .context("failed to parse tip height")?;

    let url = format!("https://litecoinspace.org/api/address/{address}/utxo");
    let utxos: Vec<Utxo> = match client
        .get(&url)
        .send()
        .await
        .context("failed to reach Litecoin Space (utxos)")?
        .error_for_status()
        .context("address not found or API error")?
        .json()
        .await
        .context("failed to parse address utxos")
    {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("{e}");
            return Err(e);
        }
    };

    let mut total: u64 = 0;
    let mut counted: usize = 0;

    for utxo in &utxos {
        let include = if !utxo.status.confirmed {
            min_confirmations == 0
        } else {
            match utxo.status.block_height {
                Some(block_height) => {
                    let confirmations = tip_height.saturating_sub(block_height) + 1;
                    confirmations >= min_confirmations
                }
                None => {
                    tracing::warn!(
                        txid = %utxo.txid,
                        vout = utxo.vout,
                        "confirmed UTXO missing block_height; skipping",
                    );
                    false
                }
            }
        };

        if !include {
            continue;
        }

        total = total
            .checked_add(utxo.value)
            .ok_or_else(|| anyhow::anyhow!("balance overflow while summing UTXOs"))?;
        counted += 1;
    }

    tracing::debug!(
        utxos_total = utxos.len(),
        utxos_counted = counted,
        min_confirmations,
        tip_height,
        "Balance: {total}",
    );

    Ok(LTCAtomicUnit::from_atomic(total))
}
