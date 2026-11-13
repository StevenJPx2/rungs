use core::fmt;

use crate::plan::{MAX_OUTCOMES, MAX_STEPS};
use crate::{Outcome, Status};

/// A plan `Plan::new` refuses, before any run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlanError {
    NoSteps,
    TooManySteps,
    Outcomes {
        step: usize,
        outcomes: u8,
    },
    NoAttempts {
        step: usize,
    },
    NotEarlier {
        step: usize,
        need: usize,
    },
    NoOutcomes {
        step: usize,
        need: usize,
    },
    NoSuchOutcome {
        step: usize,
        need: usize,
        outcome: Outcome,
    },
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSteps => write!(f, "a plan needs at least one step"),
            Self::TooManySteps => write!(f, "a plan has at most {MAX_STEPS} steps"),
            Self::Outcomes { step, outcomes } => write!(
                f,
                "step {step} has {outcomes} outcomes; a step has 1 to {MAX_OUTCOMES}"
            ),
            Self::NoAttempts { step } => write!(f, "step {step} has no attempts"),
            Self::NotEarlier { step, need } => {
                write!(f, "step {step} needs step {need}, which is not earlier")
            }
            Self::NoOutcomes { step, need } => {
                write!(
                    f,
                    "step {step} needs an empty set of step {need}'s outcomes"
                )
            }
            Self::NoSuchOutcome {
                step,
                need,
                outcome,
            } => write!(
                f,
                "step {step} needs outcome {} of step {need}, which has no such outcome",
                outcome.0
            ),
        }
    }
}

impl core::error::Error for PlanError {}

/// A call a run refuses; the run is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunError {
    NoSuchStep { step: usize },
    WrongStatus { step: usize, status: Status },
    NoSuchOutcome { step: usize, outcome: Outcome },
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchStep { step } => write!(f, "the plan has no step {step}"),
            Self::WrongStatus { step, status } => write!(f, "step {step} is {status:?}"),
            Self::NoSuchOutcome { step, outcome } => {
                write!(f, "step {step} has no outcome {}", outcome.0)
            }
        }
    }
}

impl core::error::Error for RunError {}

/// A saved progress the plan could not have reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResumeError {
    PastLastStep { step: usize },
    TooManyFailures { step: usize },
    FailedEarly { step: usize },
    NoSuchOutcome { step: usize, outcome: Outcome },
    NeedsUnmet { step: usize },
}

impl fmt::Display for ResumeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PastLastStep { step } => {
                write!(
                    f,
                    "step {step} is past the plan's last step but has progress"
                )
            }
            Self::TooManyFailures { step } => {
                write!(f, "step {step} failed more times than its attempts")
            }
            Self::FailedEarly { step } => {
                write!(
                    f,
                    "step {step} settled failed before its attempts were spent"
                )
            }
            Self::NoSuchOutcome { step, outcome } => {
                write!(
                    f,
                    "step {step} settled outcome {}, which it doesn't have",
                    outcome.0
                )
            }
            Self::NeedsUnmet { step } => {
                write!(f, "step {step} started though its needs don't hold")
            }
        }
    }
}

impl core::error::Error for ResumeError {}
