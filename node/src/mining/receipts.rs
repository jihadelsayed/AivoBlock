//! Locally verified work receipts; NOT consensus authorization or rewards.
//!
//! Only the caller that independently authenticates the assignment and
//! verifies the AI result may invoke `record_verified`. The registry does
//! not perform cryptographic worker authentication or credit tokens.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use super::scheduler::{Assignment, WorkRange};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedWorkReceipt {
    pub puzzle_id: [u8; 32],
    pub worker_id: [u8; 32],
    pub range: WorkRange,
    pub result_commitment: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptError {
    InvalidCommitment,
    InvalidRange,
    AssignmentMismatch,
    DuplicateOrOverlappingRange,
}

#[derive(Debug, Default)]
pub struct ReceiptRegistry {
    receipts: BTreeMap<([u8; 32], u64), VerifiedWorkReceipt>,
}

/// Canonical, domain-separated V1 receipt ID shared with the Haskell core.
/// The identity binds chain, puzzle, worker, unit range and result hash.
/// Caller-provided bytes do not imply that the work has been verified.
pub fn canonical_receipt_id_v1(
    chain_id: u32,
    receipt: &VerifiedWorkReceipt,
) -> Result<[u8; 32], ReceiptError> {
    if chain_id == 0 || receipt.range.count == 0 {
        return Err(ReceiptError::InvalidRange);
    }
    receipt
        .range
        .start
        .checked_add(receipt.range.count)
        .ok_or(ReceiptError::InvalidRange)?;
    let mut hash = Sha256::new();
    hash.update(b"AIVOBLOCK-VERIFIED-WORK-RECEIPT-V1\0");
    hash.update(chain_id.to_be_bytes());
    hash.update(receipt.puzzle_id);
    hash.update(receipt.worker_id);
    hash.update(receipt.range.start.to_be_bytes());
    hash.update(receipt.range.count.to_be_bytes());
    hash.update(receipt.result_commitment);
    Ok(hash.finalize().into())
}

impl ReceiptRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a result only after external authentication and recomputation.
    /// An assignment must be obtained from a trusted local scheduler; a
    /// receipt supplied by an untrusted peer is insufficient evidence.
    pub fn record_verified(
        &mut self,
        assignment: &Assignment,
        receipt: VerifiedWorkReceipt,
    ) -> Result<(), ReceiptError> {
        if receipt.puzzle_id != assignment.puzzle_id
            || receipt.worker_id != assignment.worker_id
            || receipt.range != assignment.range
        {
            return Err(ReceiptError::AssignmentMismatch);
        }
        if receipt.result_commitment == [0; 32] {
            return Err(ReceiptError::InvalidCommitment);
        }
        let end = receipt
            .range
            .start
            .checked_add(receipt.range.count)
            .ok_or(ReceiptError::InvalidRange)?;
        if receipt.range.count == 0 {
            return Err(ReceiptError::InvalidRange);
        }
        for existing in self
            .receipts
            .values()
            .filter(|r| r.puzzle_id == receipt.puzzle_id)
        {
            let existing_end = existing.range.start + existing.range.count;
            if receipt.range.start < existing_end && existing.range.start < end {
                return Err(ReceiptError::DuplicateOrOverlappingRange);
            }
        }
        self.receipts
            .insert((receipt.puzzle_id, receipt.range.start), receipt);
        Ok(())
    }

    /// Membership check for the locally admitted, recomputed receipt.
    /// Only trusted local code may call record_verified; this is not a
    /// cryptographic proof suitable for remote consensus validation.
    pub fn contains_verified(&self, receipt: &VerifiedWorkReceipt) -> bool {
        self.receipts.get(&(receipt.puzzle_id, receipt.range.start)) == Some(receipt)
    }

    pub fn receipts_for(&self, puzzle_id: [u8; 32]) -> Vec<VerifiedWorkReceipt> {
        self.receipts
            .values()
            .filter(|r| r.puzzle_id == puzzle_id)
            .copied()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assignment(start: u64, count: u64) -> Assignment {
        Assignment {
            worker_id: [2; 32],
            puzzle_id: [1; 32],
            range: WorkRange { start, count },
        }
    }

    fn receipt(a: &Assignment) -> VerifiedWorkReceipt {
        VerifiedWorkReceipt {
            puzzle_id: a.puzzle_id,
            worker_id: a.worker_id,
            range: a.range,
            result_commitment: [3; 32],
        }
    }

    #[test]
    fn canonical_id_matches_haskell_reference_vector() {
        let receipt = VerifiedWorkReceipt {
            puzzle_id: [1; 32],
            worker_id: [2; 32],
            range: WorkRange { start: 0, count: 2 },
            result_commitment: [3; 32],
        };
        let id = canonical_receipt_id_v1(1, &receipt).unwrap();
        let hex: String = id.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(
            hex,
            "1085d690f142951fb5fc32a9979d2b8d0afb2861fcc7cfae095d1f2cf9c80db1"
        );
        let mut changed = receipt;
        changed.range.start = 1;
        assert_ne!(canonical_receipt_id_v1(1, &changed).unwrap(), id);
        assert_ne!(canonical_receipt_id_v1(2, &receipt).unwrap(), id);
        changed = receipt;
        changed.worker_id[0] ^= 1;
        assert_ne!(canonical_receipt_id_v1(1, &changed).unwrap(), id);
        changed = receipt;
        changed.result_commitment[0] ^= 1;
        assert_ne!(canonical_receipt_id_v1(1, &changed).unwrap(), id);
    }

    #[test]
    fn canonical_id_rejects_invalid_ranges_and_chain() {
        let mut r = receipt(&assignment(0, 2));
        assert_eq!(
            canonical_receipt_id_v1(0, &r),
            Err(ReceiptError::InvalidRange)
        );
        r.range.count = 0;
        assert_eq!(
            canonical_receipt_id_v1(1, &r),
            Err(ReceiptError::InvalidRange)
        );
        r.range.start = u64::MAX;
        r.range.count = 2;
        assert_eq!(
            canonical_receipt_id_v1(1, &r),
            Err(ReceiptError::InvalidRange)
        );
    }

    #[test]
    fn records_disjoint_receipts() {
        let mut registry = ReceiptRegistry::new();
        for a in [assignment(0, 2), assignment(2, 1)] {
            registry.record_verified(&a, receipt(&a)).unwrap();
        }
        assert_eq!(registry.receipts_for([1; 32]).len(), 2);
    }

    #[test]
    fn rejects_duplicates_and_overlap() {
        let mut registry = ReceiptRegistry::new();
        let first = assignment(0, 2);
        registry.record_verified(&first, receipt(&first)).unwrap();
        assert_eq!(
            registry.record_verified(&first, receipt(&first)),
            Err(ReceiptError::DuplicateOrOverlappingRange)
        );
        let overlapping = assignment(1, 2);
        assert_eq!(
            registry.record_verified(&overlapping, receipt(&overlapping)),
            Err(ReceiptError::DuplicateOrOverlappingRange)
        );
    }

    #[test]
    fn rejects_forged_assignment_or_commitment() {
        let mut registry = ReceiptRegistry::new();
        let a = assignment(0, 2);
        let mut wrong = receipt(&a);
        wrong.worker_id = [9; 32];
        assert_eq!(
            registry.record_verified(&a, wrong),
            Err(ReceiptError::AssignmentMismatch)
        );
        let mut zero = receipt(&a);
        zero.result_commitment = [0; 32];
        assert_eq!(
            registry.record_verified(&a, zero),
            Err(ReceiptError::InvalidCommitment)
        );
    }

    #[test]
    fn rejects_zero_length_and_overflow() {
        let mut registry = ReceiptRegistry::new();
        for a in [assignment(0, 0), assignment(u64::MAX, 2)] {
            assert_eq!(
                registry.record_verified(&a, receipt(&a)),
                Err(ReceiptError::InvalidRange)
            );
        }
    }
}
