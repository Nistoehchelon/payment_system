use cryptocurrencies_stuff::derive_ltc::derive_ltc_bip84_address;

use payment_system_structs::PaymentMethod;

use payment_system_structs::PaymentSystemConfig;

use sqlx::PgConnection;

use payment_system_structs::PaymentId;

use payment_system_structs::ReceivingPaymentAddress;

use payment_system_structs::AddressIndex;

use crate::monero::get_monero_address_and_payment_id;

pub enum GeneratedAddress {
    Litecoin {
        index: AddressIndex,
        address: ReceivingPaymentAddress,
    },
    Monero {
        monero_payment_id: PaymentId,
        address: ReceivingPaymentAddress,
    },
}

pub async fn dynamic_address_generation(
    _payment_id: String,
    conn: &mut PgConnection,
    psc: &PaymentSystemConfig,
    m: &PaymentMethod,
) -> anyhow::Result<GeneratedAddress, String> {
    match m {
        PaymentMethod::Litecoin => {
            let index = match sqlx::query_scalar!(
                "SELECT COUNT(*) FROM invoices WHERE method = $1",
                m.to_string()
            )
            .fetch_one(&mut *conn)
            .await
            {
                Ok(Some(i)) => i,
                Err(e) => {
                    tracing::debug!("{e}");
                    return Err(format!("{e}"));
                }
                Ok(None) => {
                    tracing::debug!("dynamic_address_generation LTC 0 invoices");
                    return Err("dynamic_address_generation LTC 0 invoices".to_string());
                }
            };

            let index = index as u64;
            let address = derive_ltc_bip84_address(&psc.litecoin_master_seed, index)?;
            Ok(GeneratedAddress::Litecoin {
                index: AddressIndex(index),
                address: ReceivingPaymentAddress(address),
            })
        }
        PaymentMethod::Monero => {
            let (address, monero_payment_id) =
                match get_monero_address_and_payment_id(&psc.monero_wallet_rpc_url).await {
                    Ok(a) => a,
                    Err(e) => {
                        return Err(format!("Err '{e}' from monero subaddr gen"));
                    }
                };
            Ok(GeneratedAddress::Monero {
                monero_payment_id,
                address,
            })
        }
    }
}
