use anyhow::Context;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Utxo {
    pub txid: String,
    pub vout: u32,
    pub value: u64,
    pub status: UtxoStatus,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct UtxoStatus {
    pub confirmed: bool,
    #[serde(default)]
    pub block_height: Option<u64>,
}

pub async fn get_utxos(address: &str) -> anyhow::Result<Vec<Utxo>> {
    let url = format!("https://litecoinspace.org/api/address/{address}/utxo");
    let utxos: Vec<Utxo> = reqwest::get(&url)
        .await
        .context("failed to reach UTXO endpoint")?
        .error_for_status()
        .context("UTXO endpoint returned an error")?
        .json()
        .await
        .context("failed to parse UTXOs")?;

    Ok(utxos)
}
