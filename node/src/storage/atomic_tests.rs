//! Reopen and failure-atomicity tests; not a power-loss fault-injection test.
use super::{Column, KeyValueStore, LedgerState, LocalAccount, RocksDbStore};
use super::rocksdb::{SettlementBatch, StorageError};

fn path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("aivoblock-atomic-{label}-{}-{:?}", std::process::id(), std::thread::current().id()))
}

fn genesis() -> LedgerState {
    let mut s = LedgerState { fee_pool: 5, ..LedgerState::default() };
    s.accounts.insert([1; 32], LocalAccount { balance: 100, nonce: 0 });
    s
}

fn paid() -> LedgerState {
    let mut s = genesis();
    s.fee_pool = 0;
    s.accounts.insert([2; 32], LocalAccount { balance: 3, nonce: 0 });
    s.accounts.insert([3; 32], LocalAccount { balance: 2, nonce: 0 });
    s.settled_batches.insert([9; 32]);
    s.paid_receipts.extend([[6; 32], [7; 32]]);
    s
}

fn add_genesis(db: &RocksDbStore) {
    db.append_atomic_ledger_block(&[1; 32], &[0; 32], 0, b"genesis", &genesis(), None).unwrap();
}

fn add_payment(db: &RocksDbStore) {
    let ids = [[6; 32], [7; 32]];
    let batch = SettlementBatch { batch_id: [9; 32], receipt_ids: &ids, validated_response: b"fixture" };
    db.append_atomic_ledger_block(&[2; 32], &[1; 32], 1, b"child", &paid(), Some(&batch)).unwrap();
}

#[test]
fn atomic_block_ledger_fee_pool_and_receipts_survive_reopen() {
    let dir = path("reopen");
    let _ = std::fs::remove_dir_all(&dir);
    {
        let db = RocksDbStore::open(&dir).unwrap();
        assert_eq!(db.read_local_ledger_state().unwrap(), None);
        add_genesis(&db);
        add_payment(&db);
        assert_eq!(db.read_local_ledger_state().unwrap(), Some(paid()));
    }
    {
        let db = RocksDbStore::open(&dir).unwrap();
        assert_eq!(db.read_local_ledger_state().unwrap(), Some(paid()));
        assert_eq!(db.get(Column::ChainState, b"tip").unwrap(), Some(vec![2; 32]));
        assert_eq!(db.get(Column::ChainState, b"height").unwrap(), Some(1u64.to_be_bytes().to_vec()));
        assert_eq!(db.get(Column::Blocks, &[2; 32]).unwrap(), Some(b"child".to_vec()));
        let mut batch_key = b"reward-batch-v1:".to_vec();
        batch_key.extend_from_slice(&[9; 32]);
        assert_eq!(db.get(Column::Metadata, &batch_key).unwrap(), Some(b"fixture".to_vec()));
        for id in [[6; 32], [7; 32]] {
            let mut key = b"paid-receipt-v1:".to_vec();
            key.extend_from_slice(&id);
            assert_eq!(db.get(Column::Metadata, &key).unwrap(), Some(vec![1]));
        }
        let mut next = paid();
        next.accounts.get_mut(&[1; 32]).unwrap().balance = 89;
        next.accounts.get_mut(&[1; 32]).unwrap().nonce = 1;
        next.accounts.get_mut(&[2; 32]).unwrap().balance = 13;
        next.fee_pool = 1;
        db.append_atomic_ledger_block(&[3; 32], &[2; 32], 2, b"third", &next, None).unwrap();
        assert_eq!(db.read_local_ledger_state().unwrap(), Some(next));
    }
    {
        let db = RocksDbStore::open(&dir).unwrap();
        assert_eq!(db.get(Column::ChainState, b"tip").unwrap(), Some(vec![3; 32]));
        assert_eq!(db.read_local_ledger_state().unwrap().unwrap().fee_pool, 1);
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn rejected_parent_height_and_inflation_leave_block_and_tip_unchanged() {
    let dir = path("invalid");
    let _ = std::fs::remove_dir_all(&dir);
    {
        let db = RocksDbStore::open(&dir).unwrap();
        add_genesis(&db);
        assert!(matches!(
            db.append_atomic_ledger_block(&[2; 32], &[8; 32], 1, b"bad", &genesis(), None),
            Err(StorageError::WrongParent)
        ));
        assert!(matches!(
            db.append_atomic_ledger_block(&[2; 32], &[1; 32], 3, b"bad", &genesis(), None),
            Err(StorageError::WrongHeight)
        ));
        let mut inflated = genesis();
        inflated.accounts.get_mut(&[1; 32]).unwrap().balance = 101;
        assert!(matches!(
            db.append_atomic_ledger_block(&[2; 32], &[1; 32], 1, b"bad", &inflated, None),
            Err(StorageError::InvalidLedgerTransition)
        ));
        assert_eq!(db.read_local_ledger_state().unwrap(), Some(genesis()));
        assert_eq!(db.get(Column::Blocks, &[2; 32]).unwrap(), None);
        assert_eq!(db.get(Column::ChainState, b"tip").unwrap(), Some(vec![1; 32]));
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn rejects_batch_replay_paid_receipt_and_missing_replay_set_without_partial_writes() {
    let dir = path("replay");
    let _ = std::fs::remove_dir_all(&dir);
    {
        let db = RocksDbStore::open(&dir).unwrap();
        add_genesis(&db);
        let ids = [[6; 32], [7; 32]];
        let claim = SettlementBatch { batch_id: [9; 32], receipt_ids: &ids, validated_response: b"fixture" };
        let mut missing = paid();
        missing.paid_receipts.clear();
        assert!(matches!(
            db.append_atomic_ledger_block(&[2; 32], &[1; 32], 1, b"bad", &missing, Some(&claim)),
            Err(StorageError::InvalidLedgerTransition)
        ));
        assert_eq!(db.get(Column::Blocks, &[2; 32]).unwrap(), None);
        assert_eq!(db.read_local_ledger_state().unwrap(), Some(genesis()));
        add_payment(&db);
        let replay_batch = SettlementBatch {
            batch_id: [9; 32], receipt_ids: &[[8; 32]], validated_response: b"replay",
        };
        assert!(matches!(
            db.append_atomic_ledger_block(
                &[3; 32], &[2; 32], 2, b"bad", &paid(), Some(&replay_batch)
            ),
            Err(StorageError::DuplicateRewardBatch)
        ));
        let replay_receipt = SettlementBatch {
            batch_id: [10; 32], receipt_ids: &[[6; 32]], validated_response: b"replay",
        };
        assert!(matches!(
            db.append_atomic_ledger_block(
                &[3; 32], &[2; 32], 2, b"bad", &paid(), Some(&replay_receipt)
            ),
            Err(StorageError::AlreadyPaidReceipt)
        ));
        assert_eq!(db.read_local_ledger_state().unwrap(), Some(paid()));
        assert_eq!(db.get(Column::Blocks, &[3; 32]).unwrap(), None);
        assert_eq!(db.get(Column::ChainState, b"tip").unwrap(), Some(vec![2; 32]));
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn corrupted_snapshot_blocks_recovery_and_next_append() {
    let dir = path("corrupt");
    let _ = std::fs::remove_dir_all(&dir);
    {
        let db = RocksDbStore::open(&dir).unwrap();
        add_genesis(&db);
        db.put(Column::ChainState, b"local-ledger-snapshot-v1", b"tampered")
            .unwrap();
        assert!(matches!(
            db.read_local_ledger_state(), Err(StorageError::CorruptChainState)
        ));
        assert!(matches!(
            db.append_atomic_ledger_block(
                &[2; 32], &[1; 32], 1, b"child", &genesis(), None
            ),
            Err(StorageError::CorruptChainState)
        ));
        assert_eq!(db.get(Column::Blocks, &[2; 32]).unwrap(), None);
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn forbids_legacy_mutators_on_atomic_chain_and_legacy_seed_on_new_chain() {
    let dir = path("legacy");
    let _ = std::fs::remove_dir_all(&dir);
    {
        let db = RocksDbStore::open(&dir).unwrap();
        add_genesis(&db);
        assert!(matches!(
            db.append_checked_block(&[2; 32], &[1; 32], 1, b"legacy"),
            Err(StorageError::IncompatibleStorageMode)
        ));
        assert!(matches!(
            db.put_validated_block_and_tip(&[2; 32], b"legacy"),
            Err(StorageError::IncompatibleStorageMode)
        ));
        assert!(matches!(
            db.persist_settlement_replay_guard(&[9; 32], &[[6; 32]], b"legacy"),
            Err(StorageError::IncompatibleStorageMode)
        ));
    }
    let _ = std::fs::remove_dir_all(&dir);
    let dir = path("legacy-seed");
    let _ = std::fs::remove_dir_all(&dir);
    {
        let db = RocksDbStore::open(&dir).unwrap();
        db.persist_settlement_replay_guard(&[9; 32], &[[6; 32]], b"legacy")
            .unwrap();
        assert!(matches!(
            db.append_atomic_ledger_block(
                &[1; 32], &[0; 32], 0, b"genesis", &genesis(), None
            ),
            Err(StorageError::IncompatibleStorageMode)
        ));
    }
    let _ = std::fs::remove_dir_all(dir);
}
