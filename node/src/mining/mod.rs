//! Collaborative AI mining module for AivoBlock.
//!
//! Mining workers execute assigned AI puzzle units. A reported completion
//! is never automatically a valid proof or permission to produce a block:
//! verification and consensus are separate protocol steps.

pub mod scheduler;

pub use scheduler::{
    Assignment, AssignmentError, MiningScheduler, PuzzleTask, WorkRange,
};

/// Maximum number of puzzle work units assigned in one batch.
/// This is a local scheduling bound, not a consensus parameter.
pub const DEFAULT_MAX_BATCH_UNITS: u64 = 1024;

/// Lifecycle state of a miner's local assignment.
/// Only independent protocol verification may produce a verified result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkStatus {
    Pending,
    Assigned,
    Submitted,
    Rejected,
    Verified,
}

impl WorkStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Rejected | Self::Verified)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_final_results_are_terminal() {
        assert!(!WorkStatus::Pending.is_terminal());
        assert!(!WorkStatus::Submitted.is_terminal());
        assert!(WorkStatus::Rejected.is_terminal());
        assert!(WorkStatus::Verified.is_terminal());
    }

    #[test]
    fn default_batch_limit_is_positive() {
        assert!(DEFAULT_MAX_BATCH_UNITS > 0);
    }
}
