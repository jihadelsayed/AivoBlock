//! Deterministic local scheduling of AI puzzle work ranges.
//!
//! This is node-local bookkeeping, not a consensus-critical assignment
//! mechanism. No central scheduler or trusted puzzle issuer is assumed.
//! Proof verification and reward eligibility are handled elsewhere.

use std::collections::BTreeMap;

use super::DEFAULT_MAX_BATCH_UNITS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PuzzleTask {
    pub puzzle_id: [u8; 32],
    pub total_units: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkRange {
    pub start: u64,
    pub count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Assignment {
    pub worker_id: [u8; 32],
    pub puzzle_id: [u8; 32],
    pub range: WorkRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssignmentError {
    InvalidPuzzle,
    InvalidWorker,
    InvalidBatchSize,
    UnknownPuzzle,
    PuzzleAlreadyExists,
    NoWorkRemaining,
}

#[derive(Debug, Clone)]
struct PuzzleProgress {
    total_units: u64,
    next_unit: u64,
}

/// Assigns non-overlapping contiguous ranges to workers on this node.
/// Peer coordination, leases, retries and Byzantine fault tolerance
/// require additional protocol design.
#[derive(Debug, Default)]
pub struct MiningScheduler {
    puzzles: BTreeMap<[u8; 32], PuzzleProgress>,
}

impl MiningScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_puzzle(&mut self, task: PuzzleTask) -> Result<(), AssignmentError> {
        if task.puzzle_id == [0; 32] || task.total_units == 0 {
            return Err(AssignmentError::InvalidPuzzle);
        }
        if self.puzzles.contains_key(&task.puzzle_id) {
            return Err(AssignmentError::PuzzleAlreadyExists);
        }
        self.puzzles.insert(task.puzzle_id, PuzzleProgress {
            total_units: task.total_units,
            next_unit: 0,
        });
        Ok(())
    }

    pub fn assign(
        &mut self,
        puzzle_id: [u8; 32],
        worker_id: [u8; 32],
        requested_units: u64,
    ) -> Result<Assignment, AssignmentError> {
        if worker_id == [0; 32] {
            return Err(AssignmentError::InvalidWorker);
        }
        if requested_units == 0 || requested_units > DEFAULT_MAX_BATCH_UNITS {
            return Err(AssignmentError::InvalidBatchSize);
        }
        let progress = self.puzzles.get_mut(&puzzle_id)
            .ok_or(AssignmentError::UnknownPuzzle)?;
        if progress.next_unit >= progress.total_units {
            return Err(AssignmentError::NoWorkRemaining);
        }
        let remaining = progress.total_units - progress.next_unit;
        let count = requested_units.min(remaining);
        let range = WorkRange { start: progress.next_unit, count };
        progress.next_unit += count;
        Ok(Assignment { worker_id, puzzle_id, range })
    }

    pub fn remaining_units(&self, puzzle_id: &[u8; 32]) -> Option<u64> {
        self.puzzles.get(puzzle_id)
            .map(|progress| progress.total_units - progress.next_unit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(units: u64) -> PuzzleTask {
        PuzzleTask { puzzle_id: [1; 32], total_units: units }
    }

    #[test]
    fn distributes_nonoverlapping_work() {
        let mut scheduler = MiningScheduler::new();
        scheduler.add_puzzle(task(5)).unwrap();
        let first = scheduler.assign([1; 32], [2; 32], 3).unwrap();
        let second = scheduler.assign([1; 32], [3; 32], 3).unwrap();
        assert_eq!(first.range, WorkRange { start: 0, count: 3 });
        assert_eq!(second.range, WorkRange { start: 3, count: 2 });
        assert_eq!(scheduler.remaining_units(&[1; 32]), Some(0));
        assert_eq!(
            scheduler.assign([1; 32], [4; 32], 1),
            Err(AssignmentError::NoWorkRemaining)
        );
    }

    #[test]
    fn rejects_invalid_inputs() {
        let mut scheduler = MiningScheduler::new();
        assert_eq!(scheduler.add_puzzle(task(0)), Err(AssignmentError::InvalidPuzzle));
        scheduler.add_puzzle(task(10)).unwrap();
        assert_eq!(scheduler.add_puzzle(task(10)), Err(AssignmentError::PuzzleAlreadyExists));
        assert_eq!(
            scheduler.assign([1; 32], [0; 32], 1),
            Err(AssignmentError::InvalidWorker)
        );
        assert_eq!(
            scheduler.assign([1; 32], [2; 32], 0),
            Err(AssignmentError::InvalidBatchSize)
        );
        assert_eq!(
            scheduler.assign([9; 32], [2; 32], 1),
            Err(AssignmentError::UnknownPuzzle)
        );
    }
}
