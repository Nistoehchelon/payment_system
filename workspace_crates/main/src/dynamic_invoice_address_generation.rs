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
    Nano {
        index: AddressIndex,
        address: ReceivingPaymentAddress,
    },
}

pub async fn dynamic_address_generation(
    _payment_id: String,
    conn: &mut PgConnection,
    psc: &PaymentSystemConfig,
    m: &PaymentMethod,
) -> anyhow::Result<GeneratedAddress> {
    let index: u64 = sqlx::query_scalar!(
        "SELECT COUNT(*) as \"count!\" FROM invoices WHERE method = $1",
        m.to_string()
    )
    .fetch_one(&mut *conn)
    .await?
    .try_into()?;
    match m {
        PaymentMethod::Litecoin => {
            let address =
                cryptocurrencies_stuff::get_ltc_address::get_address_from_mnemonic_and_index(
                    &psc.litecoin_receiving_wallet_mnemonic,
                    index,
                )
                .map_err(anyhow::Error::msg)?;
            Ok(GeneratedAddress::Litecoin {
                index: AddressIndex(index.into()),
                address: ReceivingPaymentAddress(address),
            })
        }
        PaymentMethod::Monero => {
            let (address, monero_payment_id) =
                match get_monero_address_and_payment_id(&psc.monero_wallet_rpc_url).await {
                    Ok(a) => a,
                    Err(e) => {
                        return Err(anyhow::anyhow!("Err '{e}' from monero subaddr gen"));
                    }
                };
            Ok(GeneratedAddress::Monero {
                monero_payment_id,
                address,
            })
        }
        PaymentMethod::Nano => {
            let address =
                cryptocurrencies_stuff::get_xno_address::get_address_from_mnemonic_and_index(
                    &psc.nano_receiving_wallet_mnemonic,
                    index.try_into()?,
                )
                .map_err(|e| anyhow::anyhow!("{e:#?}"))?;
            Ok(GeneratedAddress::Nano {
                index: AddressIndex(index.into()),
                address: ReceivingPaymentAddress(address),
            })
        }
    }
}
