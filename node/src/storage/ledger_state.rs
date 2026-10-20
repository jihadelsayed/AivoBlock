//! Deterministic local ledger snapshot encoding for atomic RocksDB block commits.
//!
//! This is a storage format, NOT a consensus state root or validation rule.
//! The authenticated Haskell ledger remains the authority for actual balances.

use std::collections::{BTreeMap, BTreeSet};

const MAGIC: &[u8] = b"AIVOBLOCK-LOCAL-LEDGER-V1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalAccount {
    pub balance: u64,
    pub nonce: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LedgerState {
    pub accounts: BTreeMap<[u8; 32], LocalAccount>,
    pub fee_pool: u64,
    pub settled_batches: BTreeSet<[u8; 32]>,
    pub paid_receipts: BTreeSet<[u8; 32]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateCodecError {
    MalformedSnapshot,
    TooManyEntries,
}

fn write_count(out: &mut Vec<u8>, count: usize) -> Result<(), StateCodecError> {
    let count = u32::try_from(count).map_err(|_| StateCodecError::TooManyEntries)?;
    out.extend_from_slice(&count.to_be_bytes());
    Ok(())
}

/// Encode strictly ordered maps/sets using fixed-width network-endian fields.
/// The byte layout is domain || fee_pool:u64 || account_count:u32 ||
/// [address:32, balance:u64, nonce:u64] || batch_count:u32 || [batch_id:32]
/// || receipt_count:u32 || [receipt_id:32].
pub fn encode_ledger_state(state: &LedgerState) -> Result<Vec<u8>, StateCodecError> {
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&state.fee_pool.to_be_bytes());
    write_count(&mut out, state.accounts.len())?;
    for (address, account) in &state.accounts {
        out.extend_from_slice(address);
        out.extend_from_slice(&account.balance.to_be_bytes());
        out.extend_from_slice(&account.nonce.to_be_bytes());
    }
    write_count(&mut out, state.settled_batches.len())?;
    for batch in &state.settled_batches {
        out.extend_from_slice(batch);
    }
    write_count(&mut out, state.paid_receipts.len())?;
    for receipt in &state.paid_receipts {
        out.extend_from_slice(receipt);
    }
    Ok(out)
}

fn read_bytes<const N: usize>(
    bytes: &[u8],
    offset: &mut usize,
) -> Result<[u8; N], StateCodecError> {
    let end = offset
        .checked_add(N)
        .ok_or(StateCodecError::MalformedSnapshot)?;
    let value = bytes
        .get(*offset..end)
        .ok_or(StateCodecError::MalformedSnapshot)?;
    let mut result = [0u8; N];
    result.copy_from_slice(value);
    *offset = end;
    Ok(result)
}

fn read_count(
    bytes: &[u8],
    offset: &mut usize,
    item_size: usize,
) -> Result<usize, StateCodecError> {
    let count = u32::from_be_bytes(read_bytes::<4>(bytes, offset)?) as usize;
    if count > bytes.len().saturating_sub(*offset) / item_size {
        return Err(StateCodecError::MalformedSnapshot);
    }
    Ok(count)
}

/// Reject truncated data, reordered/duplicate entries, and trailing bytes.
/// A canonical round-trip prevents two encodings of the same state.
pub fn decode_ledger_state(bytes: &[u8]) -> Result<LedgerState, StateCodecError> {
    if !bytes.starts_with(MAGIC) {
        return Err(StateCodecError::MalformedSnapshot);
    }
    let mut offset = MAGIC.len();
    let fee_pool = u64::from_be_bytes(read_bytes::<8>(bytes, &mut offset)?);
    let mut accounts = BTreeMap::new();
    for _ in 0..read_count(bytes, &mut offset, 48)? {
        let address = read_bytes::<32>(bytes, &mut offset)?;
        let balance = u64::from_be_bytes(read_bytes::<8>(bytes, &mut offset)?);
        let nonce = u64::from_be_bytes(read_bytes::<8>(bytes, &mut offset)?);
        if accounts
            .insert(address, LocalAccount { balance, nonce })
            .is_some()
        {
            return Err(StateCodecError::MalformedSnapshot);
        }
    }
    let mut settled_batches = BTreeSet::new();
    for _ in 0..read_count(bytes, &mut offset, 32)? {
        if !settled_batches.insert(read_bytes::<32>(bytes, &mut offset)?) {
            return Err(StateCodecError::MalformedSnapshot);
        }
    }
    let mut paid_receipts = BTreeSet::new();
    for _ in 0..read_count(bytes, &mut offset, 32)? {
        if !paid_receipts.insert(read_bytes::<32>(bytes, &mut offset)?) {
            return Err(StateCodecError::MalformedSnapshot);
        }
    }
    let state = LedgerState {
        accounts,
        fee_pool,
        settled_batches,
        paid_receipts,
    };
    if offset != bytes.len() || encode_ledger_state(&state)? != bytes {
        return Err(StateCodecError::MalformedSnapshot);
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> LedgerState {
        let mut ledger = LedgerState {
            fee_pool: 13,
            ..LedgerState::default()
        };
        ledger.accounts.insert(
            [3; 32],
            LocalAccount {
                balance: 20,
                nonce: 1,
            },
        );
        ledger.accounts.insert(
            [1; 32],
            LocalAccount {
                balance: 40,
                nonce: 2,
            },
        );
        ledger.settled_batches.insert([9; 32]);
        ledger.paid_receipts.insert([5; 32]);
        ledger
    }

    #[test]
    fn round_trip_is_deterministic() {
        let state = example();
        let encoded = encode_ledger_state(&state).unwrap();
        assert_eq!(decode_ledger_state(&encoded), Ok(state.clone()));
        let mut reordered = LedgerState::default();
        for (address, account) in state.accounts.iter().rev() {
            reordered.accounts.insert(*address, *account);
        }
        reordered.fee_pool = state.fee_pool;
        reordered.settled_batches = state.settled_batches;
        reordered.paid_receipts = state.paid_receipts;
        assert_eq!(encode_ledger_state(&reordered).unwrap(), encoded);
    }

    #[test]
    fn rejects_truncation_corruption_and_extra_bytes() {
        let encoded = encode_ledger_state(&example()).unwrap();
        for truncated in 0..encoded.len() {
            assert_eq!(
                decode_ledger_state(&encoded[..truncated]),
                Err(StateCodecError::MalformedSnapshot)
            );
        }
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert_eq!(
            decode_ledger_state(&trailing),
            Err(StateCodecError::MalformedSnapshot)
        );
        let mut fake_count = encoded;
        fake_count[MAGIC.len() + 8..MAGIC.len() + 12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(
            decode_ledger_state(&fake_count),
            Err(StateCodecError::MalformedSnapshot)
        );
    }
}
