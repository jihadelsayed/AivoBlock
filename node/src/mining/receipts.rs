//! Locally verified work receipts; NOT consensus authorization or rewards.
//!
//! Only the caller that independently authenticates the assignment and
//! verifies the AI result may invoke `record_verified`. The registry does
//! not perform cryptographic worker authentication or credit tokens.

use std::collections::BTreeMap;

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
