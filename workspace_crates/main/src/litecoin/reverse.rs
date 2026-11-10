use std::str::FromStr;

use bech32::Hrp;
use bip39::Mnemonic;
use bitcoin::bip32::{ChildNumber, DerivationPath, Xpriv};
use bitcoin::secp256k1::Secp256k1;
use ripemd::Ripemd160;
use sha2::{Digest, Sha256};

/// Default scan window. BIP44 defines a gap limit of 20; recovering an
/// unknown wallet usually needs more. 100 covers almost every real case.
pub const DEFAULT_LTC_SCAN_RANGE: u32 = 100;

/// Reverse of `derive_ltc_bip84_address` for the external chain
/// `m/84'/2'/0'/0/*`.
///
/// Returns `Ok(Some(i))` if `address` matches index `i` on this seed's
/// external chain; `Ok(None)` if nothing in `0..=max_index` matches;
/// `Err(_)` for bad input / derivation errors.
pub fn find_ltc_bip84_index(
    master_seed: &str,
    address: &str,
    max_index: u32,
) -> Result<Option<u32>, String> {
    // Non-hardened indices live in [0, 2^31). Refuse anything above that
    // so we don't scan the hardened space by accident.
    if max_index >= (1u32 << 31) {
        return Err(format!(
            "max_index {max_index} is >= 2^31; only the non-hardened range is scanned"
        ));
    }

    // 1. Parse mnemonic and derive the BIP39 seed (no passphrase).
    let mnemonic = Mnemonic::from_str(master_seed).map_err(|e| format!("Invalid mnemonic: {e}"))?;
    let seed = mnemonic.to_seed("");

    // 2. Master extended private key from the 64-byte seed.
    let secp = Secp256k1::new();
    let master = Xpriv::new_master(bitcoin::Network::Bitcoin, &seed)
        .map_err(|e| format!("Master key derivation failed: {e}"))?;

    // 3. Derive the fixed prefix m/84'/2'/0'/0 once.
    //    Only the final (non-hardened) step varies per index.
    let account_path = DerivationPath::from(vec![
        ChildNumber::from_hardened_idx(84).unwrap(), // 84' purpose
        ChildNumber::from_hardened_idx(2).unwrap(),  // 2'  coin (LTC)
        ChildNumber::from_hardened_idx(0).unwrap(),  // 0'  account
        ChildNumber::from_normal_idx(0).unwrap(),    // 0   external chain
    ]);
    let account = master
        .derive_priv(&secp, &account_path)
        .map_err(|e| format!("Account derivation failed: {e}"))?;

    // 4. Precompute HRP and normalize the target (bech32 is
    //    case-insensitive; encode_v0 always emits lowercase).
    let hrp = Hrp::parse("ltc").map_err(|e| format!("Invalid HRP: {e}"))?;
    let target = address.trim().to_lowercase();

    // 5. Scan: one non-hardened derivation + one HASH160 per index.
    for index in 0..=max_index {
        let child_number = ChildNumber::from_normal_idx(index)
            .map_err(|e| format!("invalid index {index}: {e}"))?;

        // `derive_priv` takes a full path; a one-element path = one step.
        let child_path = DerivationPath::from(vec![child_number]);
        let child = account
            .derive_priv(&secp, &child_path)
            .map_err(|e| format!("Derive index {index} failed: {e}"))?;

        // Compressed pubkey (33 bytes) → HASH160 = RIPEMD160(SHA256(...)).
        let pubkey = child.private_key.public_key(&secp);
        let pubkey_bytes = pubkey.serialize(); // [u8; 33], compressed

        let sha256 = Sha256::digest(pubkey_bytes);
        let hash160 = Ripemd160::digest(sha256);

        let candidate = bech32::segwit::encode_v0(hrp, &hash160)
            .map_err(|e| format!("bech32 encode failed: {e}"))?;

        if candidate == target {
            return Ok(Some(index));
        }
    }

    Ok(None)
}

/// Convenience wrapper using `DEFAULT_LTC_SCAN_RANGE`.
pub fn find_ltc_bip84_index_default(
    master_seed: &str,
    address: &str,
) -> Result<Option<u32>, String> {
    find_ltc_bip84_index(master_seed, address, DEFAULT_LTC_SCAN_RANGE)
}
