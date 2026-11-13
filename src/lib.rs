//! A step machine for plans whose steps wait on the outcomes of earlier
//! steps. No I/O, no dependencies and no allocation.
//!
//! A [`Plan`] is up to 16 steps, each with its outcomes, its needs on
//! earlier steps and its attempts. A [`Run`] borrows the plan and says which
//! steps to start, which are skipped, and when the run is over; its
//! [`Progress`] is plain data the program saves and resumes.
//!
//! ```
//! use rungs::{Failed, Need, Outcome, Plan, State, Step};
//!
//! // fast, then sure only when fast was unsure (1) or failed
//! let plan = Plan::new([
//!     Step::new(2),
//!     Step::new(2).after([Need::outcome(0, [Outcome(1), Outcome::FAILED])]),
//! ])?;
//!
//! let mut run = plan.start();
//! assert_eq!(run.advance(), [0]);
//!
//! run.settle(0, Outcome(1))?;
//! assert_eq!(run.advance(), [1]);
//! assert_eq!(run.fail(1)?, Failed::Final);
//!
//! let saved = run.progress();
//! let run = plan.resume(saved)?;
//!
//! assert_eq!(run.state(), State::Over { unhandled: Some(1) });
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![no_std]

mod error;
mod need;
mod plan;
mod progress;
mod run;

#[cfg(kani)]
mod narrow;
#[cfg(kani)]
mod proofs;

pub use error::{PlanError, ResumeError, RunError};
pub use need::{Need, Outcome, Outcomes, Wants};
pub use plan::{MAX_OUTCOMES, MAX_STEPS, Plan, Step};
pub use progress::{Progress, Status};
pub use run::{Failed, Run, State, Steps, StepsIter};
