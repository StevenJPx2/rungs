use core::fmt;

use crate::plan::step_bit;
use crate::{MAX_STEPS, Outcome, Plan, Progress, ResumeError, RunError, Status, Step};

/// A run of a plan: the plan it borrows and its progress. `Copy`, so a
/// program can try a batch of calls on a copy and keep it only if each one
/// succeeds.
///
/// ```
/// use rungs::{Plan, Status, Step};
///
/// let plan = Plan::new([Step::new(1)])?;
/// let run = plan.start();
///
/// assert_eq!(run.status(0), Some(Status::Pending));
/// assert_eq!(run.progress().failed[0], 0);
/// # Ok::<(), rungs::PlanError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Run<'plan> {
    plan: &'plan Plan,
    progress: Progress,
}

impl Plan {
    /// A fresh run: every step Pending, none failed.
    #[must_use]
    pub fn start(&self) -> Run<'_> {
        Run {
            plan: self,
            progress: Progress::default(),
        }
    }

    /// A run that goes on from a saved progress.
    ///
    /// # Errors
    ///
    /// Refuses a progress this plan could not have reached: a step past the
    /// plan that isn't Pending with none failed, more failed attempts than
    /// the step has, `Outcome::FAILED` before its attempts are spent, an
    /// outcome the step doesn't have, and a step running or settled whose
    /// needs don't hold.
    pub fn resume(&self, progress: Progress) -> Result<Run<'_>, ResumeError> {
        let run = Run {
            plan: self,
            progress,
        };

        let positions = progress.status.iter().zip(progress.failed).enumerate();

        for (index, (status, failed)) in positions {
            run.check(index, *status, failed)?;
        }

        Ok(run)
    }
}

impl<'plan> Run<'plan> {
    #[must_use]
    pub const fn plan(&self) -> &'plan Plan {
        self.plan
    }

    /// The progress so far, to save.
    #[must_use]
    pub const fn progress(&self) -> Progress {
        self.progress
    }

    /// Step `index`'s status; none past the plan's last step.
    #[must_use]
    pub fn status(&self, index: usize) -> Option<Status> {
        if index < self.plan.steps() {
            self.progress.status.get(index).copied()
        } else {
            None
        }
    }

    /// Decides every Pending step whose named steps have all finished, in
    /// order, so a skip carries through in the same pass: a step whose needs
    /// hold starts, and the rest are skipped. Returns the steps it started.
    pub fn advance(&mut self) -> Steps {
        let mut started = Steps::default();

        for index in 0..MAX_STEPS {
            if self.status(index) != Some(Status::Pending) {
                continue;
            }

            match self.decide(index) {
                Some(true) => {
                    self.set(index, Status::Running);
                    started.insert(index);
                }
                Some(false) => self.set(index, Status::Skipped),
                None => {}
            }
        }

        started
    }

    /// Settles a running step with one of its own outcomes.
    ///
    /// # Errors
    ///
    /// Refuses a step past the plan, one that isn't running, and an outcome
    /// the step doesn't have, `Outcome::FAILED` included: that comes from
    /// `fail`. A refused call changes nothing.
    pub fn settle(&mut self, index: usize, outcome: Outcome) -> Result<(), RunError> {
        let step = self.expect(index, Status::Running)?;

        if outcome.0 >= step.outcome_count() {
            return Err(RunError::NoSuchOutcome {
                step: index,
                outcome,
            });
        }

        self.set(index, Status::Settled(outcome));
        Ok(())
    }

    /// Counts a failed attempt of a running step. Under its attempts it stays
    /// running, for the program to start again; at the limit it settles
    /// `Outcome::FAILED`.
    ///
    /// # Errors
    ///
    /// Refuses a step past the plan and one that isn't running. A refused
    /// call changes nothing.
    pub fn fail(&mut self, index: usize) -> Result<Failed, RunError> {
        let attempts = self.expect(index, Status::Running)?.attempt_limit();
        let failed = self.failed(index).saturating_add(1).min(attempts);

        if let Some(slot) = self.progress.failed.get_mut(index) {
            *slot = failed;
        }

        if failed < attempts {
            return Ok(Failed::Retry(failed));
        }

        self.set(index, Status::Settled(Outcome::FAILED));
        Ok(Failed::Final)
    }

    /// Returns step `index` and every step that depends on it, directly or
    /// not, to Pending, keeping their failed attempts. Returns the steps it
    /// moved, `index` first among them.
    ///
    /// # Errors
    ///
    /// Refuses a step past the plan and a Pending one. A refused call
    /// changes nothing.
    pub fn restart(&mut self, index: usize) -> Result<Steps, RunError> {
        if self.status(index) == Some(Status::Pending) {
            return Err(RunError::WrongStatus {
                step: index,
                status: Status::Pending,
            });
        }

        if self.plan.step(index).is_none() {
            return Err(RunError::NoSuchStep { step: index });
        }

        let reach = self.plan.reach(index);
        let mut moved = Steps::default();

        for (step, status) in self.progress.status.iter_mut().enumerate() {
            let bit = step_bit(step).unwrap_or(0);

            if reach & bit != 0 && *status != Status::Pending {
                *status = Status::Pending;
                moved.0 |= bit;
            }
        }

        Ok(moved)
    }

    /// Skips a Pending step, whether or not its needs are decided, for a
    /// reason the program knows, such as a deadline. A skipped step counts
    /// as finished for `Need::settled`.
    ///
    /// # Errors
    ///
    /// Refuses a step past the plan and one that isn't Pending. A refused
    /// call changes nothing.
    pub fn skip(&mut self, index: usize) -> Result<(), RunError> {
        self.expect(index, Status::Pending)?;
        self.set(index, Status::Skipped);
        Ok(())
    }

    /// Running while a step runs or `advance` would start one; otherwise
    /// over, naming the first step whose final failure no step that started
    /// was waiting for.
    #[must_use]
    pub fn state(&self) -> State {
        let mut next = *self;
        let _started = next.advance();

        for status in next.progress.status {
            if status == Status::Running {
                return State::Running;
            }
        }

        let mut started = 0;

        for (index, status) in self.progress.status.iter().enumerate() {
            if matches!(status, Status::Running | Status::Settled(_)) {
                started |= step_bit(index).unwrap_or(0);
            }
        }

        let mut unhandled = None;

        for (index, status) in self.progress.status.iter().enumerate() {
            let failed = *status == Status::Settled(Outcome::FAILED);

            if unhandled.is_none() && failed && self.plan.handlers(index) & started == 0 {
                unhandled = Some(index);
            }
        }

        State::Over { unhandled }
    }

    /// Whether one step's saved status and failed attempts are reachable.
    fn check(&self, index: usize, status: Status, failed: u8) -> Result<(), ResumeError> {
        let Some(step) = self.plan.step(index) else {
            return if status == Status::Pending && failed == 0 {
                Ok(())
            } else {
                Err(ResumeError::PastLastStep { step: index })
            };
        };

        if failed > step.attempt_limit() {
            return Err(ResumeError::TooManyFailures { step: index });
        }

        match status {
            Status::Pending | Status::Skipped => return Ok(()),
            Status::Settled(Outcome::FAILED) if failed < step.attempt_limit() => {
                return Err(ResumeError::FailedEarly { step: index });
            }
            Status::Settled(outcome)
                if outcome != Outcome::FAILED && outcome.0 >= step.outcome_count() =>
            {
                return Err(ResumeError::NoSuchOutcome {
                    step: index,
                    outcome,
                });
            }
            Status::Running | Status::Settled(_) => {}
        }

        if self.decide(index) == Some(true) {
            Ok(())
        } else {
            Err(ResumeError::NeedsUnmet { step: index })
        }
    }

    /// Whether step `index` should start: none while a step it names is
    /// unfinished.
    fn decide(&self, index: usize) -> Option<bool> {
        self.plan.step(index)?.decide(&self.progress.status)
    }

    fn failed(&self, index: usize) -> u8 {
        self.progress.failed.get(index).copied().unwrap_or(0)
    }

    fn set(&mut self, index: usize, status: Status) {
        if let Some(slot) = self.progress.status.get_mut(index) {
            *slot = status;
        }
    }

    /// Step `index`, when it is in the status a call expects.
    fn expect(&self, index: usize, expected: Status) -> Result<&'plan Step, RunError> {
        let (Some(step), Some(status)) = (self.plan.step(index), self.status(index)) else {
            return Err(RunError::NoSuchStep { step: index });
        };

        if status == expected {
            Ok(step)
        } else {
            Err(RunError::WrongStatus {
                step: index,
                status,
            })
        }
    }
}

/// What `fail` did with a failed attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Failed {
    /// Under its attempts: the step stays running, to be started again.
    /// Holds its failed attempts so far.
    Retry(u8),
    /// Its attempts are spent: it settled `Outcome::FAILED`.
    Final,
}

/// Whether a run goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    /// A step runs or can start.
    Running,
    /// No step runs or can start. `unhandled` names a step whose final
    /// failure no started step was waiting for.
    Over { unhandled: Option<usize> },
}

/// A set of a plan's steps, such as those `advance` starts, lowest first.
///
/// ```
/// use rungs::Steps;
///
/// let none = Steps::default();
///
/// assert!(none.is_empty());
/// assert_eq!(none, []);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Steps(u16);

impl Steps {
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[must_use]
    pub fn len(self) -> usize {
        self.iter().count()
    }

    #[must_use]
    pub fn contains(self, index: usize) -> bool {
        step_bit(index).is_some_and(|bit| self.0 & bit != 0)
    }

    #[must_use]
    pub const fn iter(self) -> StepsIter {
        StepsIter {
            steps: self,
            next: 0,
        }
    }

    pub(crate) fn insert(&mut self, index: usize) {
        if let Some(bit) = step_bit(index) {
            self.0 |= bit;
        }
    }
}

/// The steps of a `Steps`, lowest first.
#[derive(Debug, Clone)]
pub struct StepsIter {
    steps: Steps,
    next: usize,
}

impl Iterator for StepsIter {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        while self.next < MAX_STEPS {
            let index = self.next;
            self.next = index.saturating_add(1);

            if self.steps.contains(index) {
                return Some(index);
            }
        }

        None
    }
}

impl IntoIterator for Steps {
    type Item = usize;
    type IntoIter = StepsIter;

    fn into_iter(self) -> StepsIter {
        self.iter()
    }
}

impl<const N: usize> PartialEq<[usize; N]> for Steps {
    fn eq(&self, other: &[usize; N]) -> bool {
        self.iter().eq(other.iter().copied())
    }
}

impl fmt::Debug for Steps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}
