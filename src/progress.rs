use crate::{MAX_STEPS, Outcome};

/// Where one step is in a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum Status {
    /// Not decided yet, or sent back by a restart.
    #[default]
    Pending,
    /// Started, and not settled.
    Running,
    /// Settled with its outcome, `Outcome::FAILED` once its attempts are spent.
    Settled(Outcome),
    /// Decided against, or skipped by the program.
    Skipped,
}

impl Status {
    /// Settled or skipped: a step that names it can be decided.
    #[must_use]
    pub const fn is_finished(self) -> bool {
        matches!(self, Self::Settled(_) | Self::Skipped)
    }
}

/// A run's progress, without its plan: each step's status and failed
/// attempts, by position. Steps past the plan's last stay Pending with none
/// failed. Plain data, for the program to save however it likes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub struct Progress {
    pub status: [Status; MAX_STEPS],
    /// Failed attempts, kept across restarts.
    pub failed: [u8; MAX_STEPS],
}
