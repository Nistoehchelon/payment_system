use cryptocurrencies_stuff::currencies::LTCAtomicUnit;
use anyhow::Context;

#[derive(Debug, serde::Deserialize)]
pub struct AddressStats {
    chain_stats: ChainStats,
}

#[derive(Debug, serde::Deserialize)]
pub struct ChainStats {
    funded_txo_sum: u64,
    spent_txo_sum: u64,
}



pub async fn get_address_balance(address: &str) -> anyhow::Result<LTCAtomicUnit> {
    let url = format!("https://litecoinspace.org/api/address/{address}");
    let stats: AddressStats = reqwest::get(&url)
        .await
        .context("failed to reach Litecoin Space")?
        .error_for_status()
        .context("address not found or API error")?
        .json()
        .await
        .context("failed to parse address stats")?;

    let confirmed = stats
        .chain_stats
        .funded_txo_sum
        .checked_sub(stats.chain_stats.spent_txo_sum)
        .ok_or_else(|| anyhow::anyhow!("address stats are inconsistent (spent > funded)"))?;

    Ok(LTCAtomicUnit::from_atomic(confirmed))
}