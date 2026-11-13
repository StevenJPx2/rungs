use crate::need::{Bits, SKIPPED, Wants, bit};
use crate::{Need, Outcome, Outcomes, PlanError, Status};

/// The most steps a plan has, so a set of steps fits in a `u16`.
#[cfg(not(kani))]
pub const MAX_STEPS: usize = 16;

// Kani checks every plan of up to four steps; room for exactly those keeps
// its formulas small. The code is the same at either size.
#[cfg(kani)]
pub const MAX_STEPS: usize = 4;

/// The most outcomes a step has besides `Outcome::FAILED`.
pub const MAX_OUTCOMES: u8 = 126;

/// One step of a plan: how many outcomes it has, what it waits for, and how
/// many times it may run.
///
/// A step keeps its needs by the step they name: two `after` needs on one
/// step hold together, so they read back as one need on the outcomes both
/// accept, and two `after_any` needs on one step as one on the outcomes
/// either accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Step {
    outcomes: u8,
    attempts: u8,
    after: Rule,
    after_any: Rule,
    /// Every outcome its needs name on each step, for `Plan::new` to check.
    named: [Bits; MAX_STEPS],
    /// The first need no plan could hold, kept for `Plan::new` to refuse.
    fault: Option<Need>,
}

impl Step {
    /// A step with `outcomes` outcomes, numbered from 0, no needs, and one
    /// attempt.
    #[must_use]
    pub const fn new(outcomes: u8) -> Self {
        Self {
            outcomes,
            attempts: 1,
            after: Rule::new(Bits::MAX),
            after_any: Rule::new(0),
            named: [0; MAX_STEPS],
            fault: None,
        }
    }

    /// Adds needs that must all hold.
    #[must_use]
    pub fn after(mut self, needs: impl IntoIterator<Item = Need>) -> Self {
        for need in needs {
            if let Some(step) = self.keep(need) {
                let accepts = need.accepts() & self.after.accepted(step);
                self.after.add(step, accepts);
            }
        }

        self
    }

    /// Adds needs of which at least one must hold.
    #[must_use]
    pub fn after_any(mut self, needs: impl IntoIterator<Item = Need>) -> Self {
        for need in needs {
            if let Some(step) = self.keep(need) {
                let accepts = need.accepts() | self.after_any.accepted(step);
                self.after_any.add(step, accepts);
            }
        }

        self
    }

    /// Adds a need on `step` accepting the codes `accepts`, as `after` or
    /// `after_any` keep it, for the proofs to range over every need.
    #[cfg(kani)]
    pub(crate) fn with_need(mut self, any: bool, step: usize, accepts: Bits) -> Self {
        if any {
            self.after_any.add(step, accepts);
        } else {
            self.after.add(step, accepts);
        }

        self
    }

    /// How many times it may run before `Outcome::FAILED` is final.
    #[must_use]
    pub const fn attempts(mut self, attempts: u8) -> Self {
        self.attempts = attempts;
        self
    }

    /// The step a need names, when a plan could hold it; otherwise the need
    /// is kept as the step's fault.
    fn keep(&mut self, need: Need) -> Option<usize> {
        let empty = matches!(need.wants(), Wants::Outcome(set) if set.is_empty());
        let slot = self.named.get_mut(need.step());

        match slot {
            Some(named) if need.stray().is_none() && !empty => {
                if let Wants::Outcome(_) = need.wants() {
                    *named |= need.accepts();
                }

                Some(need.step())
            }
            _ => {
                self.fault = self.fault.or(Some(need));
                None
            }
        }
    }

    pub(crate) const fn outcome_count(&self) -> u8 {
        self.outcomes
    }

    pub(crate) const fn attempt_limit(&self) -> u8 {
        self.attempts
    }

    /// The steps it names, in `after` or `after_any`, as a mask.
    const fn names_mask(&self) -> u16 {
        self.after.waits | self.after_any.waits
    }

    /// Whether it names step `index`, in `after` or `after_any`.
    fn names(&self, index: usize) -> bool {
        step_bit(index).is_some_and(|bit| self.names_mask() & bit != 0)
    }

    /// Whether it should start, given every step's status: none while a step
    /// it names is unfinished.
    pub(crate) fn decide(&self, statuses: &[Status; MAX_STEPS]) -> Option<bool> {
        let mut all = true;
        let mut any = self.after_any.waits == 0;
        let mut unfinished = false;
        let mut bit = 1_u16;

        let lists = self.after.accepts.iter().zip(&self.after_any.accepts);

        for (status, (all_accepts, any_accepts)) in statuses.iter().zip(lists) {
            let in_all = self.after.waits & bit != 0;
            let in_any = self.after_any.waits & bit != 0;
            let code = codes(*status);

            unfinished |= (in_all || in_any) && code == 0;
            all &= !in_all || all_accepts & code != 0;
            any |= in_any && any_accepts & code != 0;
            bit = bit.wrapping_shl(1);
        }

        if unfinished { None } else { Some(all && any) }
    }
}

/// The bit a finished step matches in a need's set: its outcome's, or
/// `SKIPPED`'s. None while it is unfinished.
fn codes(status: Status) -> Bits {
    let code = match status {
        Status::Settled(outcome) => outcome.0,
        Status::Skipped => SKIPPED,
        Status::Pending | Status::Running => return 0,
    };

    bit(code).unwrap_or(0)
}

/// One list of a step's needs: the steps it waits on, and for each the codes
/// it accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Rule {
    waits: u16,
    /// The steps whose final failure an outcome need in it names.
    fails: u16,
    accepts: [Bits; MAX_STEPS],
}

impl Rule {
    const fn new(accepts: Bits) -> Self {
        Self {
            waits: 0,
            fails: 0,
            accepts: [accepts; MAX_STEPS],
        }
    }

    /// Waits on `step`, accepting `accepts`: the caller merges it with what
    /// the list accepted already.
    fn add(&mut self, step: usize, accepts: Bits) {
        if let (Some(kept), Some(bit)) = (self.accepts.get_mut(step), step_bit(step)) {
            *kept = accepts;
            self.waits |= bit;

            let failed = Outcome::FAILED.0;
            let names_failure =
                accepts != Bits::MAX && self::bit(failed).is_some_and(|f| accepts & f != 0);

            if names_failure {
                self.fails |= bit;
            } else {
                self.fails &= !bit;
            }
        }
    }

    /// What the list accepts of `step`: everything in `after` and nothing in
    /// `after_any` until a need names it.
    fn accepted(&self, step: usize) -> Bits {
        self.accepts.get(step).copied().unwrap_or(0)
    }

    fn waits_on(&self, step: usize) -> bool {
        step_bit(step).is_some_and(|bit| self.waits & bit != 0)
    }

    fn needs(&self) -> impl Iterator<Item = Need> + '_ {
        self.accepts
            .iter()
            .enumerate()
            .filter(|(step, _)| self.waits_on(*step))
            .map(|(step, accepts)| Need::kept(step, *accepts))
    }
}

pub fn step_bit(step: usize) -> Option<u16> {
    1_u16.checked_shl(u32::try_from(step).ok()?)
}

/// Up to 16 steps, each waiting only on earlier ones, checked once.
///
/// ```
/// use rungs::{Need, Outcome, Plan, Step};
///
/// // worktree, deps, audit (found or clean), and fix
/// let plan = Plan::new([
///     Step::new(1),
///     Step::new(1).after([Need::outcome(0, [Outcome(0)])]),
///     Step::new(2).after([Need::outcome(0, [Outcome(0)])]),
///     Step::new(1).after_any([
///         Need::outcome(2, [Outcome(0)]),
///         Need::outcome(1, [Outcome::FAILED]),
///     ]),
/// ])?;
///
/// assert_eq!(plan.steps(), 4);
/// assert_eq!(plan.depth(), 3);
/// # Ok::<(), rungs::PlanError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Plan {
    steps: [Step; MAX_STEPS],
    count: usize,
    depth: usize,
    /// For each step, the steps a restart of it moves: itself and every
    /// step that depends on it, directly or not.
    reach: [u16; MAX_STEPS],
    /// For each step, the steps that wait for its final failure by name.
    handlers: [u16; MAX_STEPS],
}

impl Plan {
    /// # Errors
    ///
    /// Refuses no steps, more than 16, a step with no outcomes or more than
    /// 126 or no attempts, and a need on the step itself or a later one, on
    /// an empty set of outcomes, or on an outcome the step it names doesn't
    /// have.
    pub fn new(steps: impl IntoIterator<Item = Step>) -> Result<Self, PlanError> {
        let mut plan = Self {
            steps: [Step::new(1); MAX_STEPS],
            count: 0,
            depth: 0,
            reach: [0; MAX_STEPS],
            handlers: [0; MAX_STEPS],
        };

        for step in steps {
            let Some(slot) = plan.steps.get_mut(plan.count) else {
                return Err(PlanError::TooManySteps);
            };

            *slot = step;
            plan.count = plan.count.saturating_add(1);
        }

        if plan.count == 0 {
            return Err(PlanError::NoSteps);
        }

        for (index, step) in plan.listed().iter().enumerate() {
            plan.check(index, step)?;
        }

        plan.derive();
        Ok(plan)
    }

    /// A plan of steps the proofs build valid, without `Plan::new`'s checks,
    /// which its tests cover.
    #[cfg(kani)]
    pub(crate) fn unchecked(steps: [Step; MAX_STEPS], count: usize) -> Self {
        let mut plan = Self {
            steps,
            count,
            depth: 0,
            reach: [0; MAX_STEPS],
            handlers: [0; MAX_STEPS],
        };

        plan.derive();
        plan
    }

    /// What runs read of the plan's shape, worked out once.
    fn derive(&mut self) {
        self.depth = self.longest_chain();

        let mut bit = 1_u16;

        for (reach, handlers) in self.reach.iter_mut().zip(&mut self.handlers) {
            *reach = bit;
            let mut other = 1_u16;

            for step in &self.steps {
                if other > bit && step.names_mask() & *reach != 0 {
                    *reach |= other;
                }

                if (step.after.fails | step.after_any.fails) & bit != 0 {
                    *handlers |= other;
                }

                other = other.wrapping_shl(1);
            }

            bit = bit.wrapping_shl(1);
        }
    }

    fn check(&self, index: usize, step: &Step) -> Result<(), PlanError> {
        if !(1..=MAX_OUTCOMES).contains(&step.outcomes) {
            return Err(PlanError::Outcomes {
                step: index,
                outcomes: step.outcomes,
            });
        }

        if step.attempts == 0 {
            return Err(PlanError::NoAttempts { step: index });
        }

        if let Some(need) = step.fault {
            return Err(fault(index, need));
        }

        for (named, outcomes) in step.named.iter().enumerate() {
            if !step.names(named) {
                continue;
            }

            let Some(earlier) = self.step(named).filter(|_| named < index) else {
                return Err(PlanError::NotEarlier {
                    step: index,
                    need: named,
                });
            };

            if let Some(outcome) = Outcomes::from_bits(*outcomes).beyond(earlier.outcomes) {
                return Err(PlanError::NoSuchOutcome {
                    step: index,
                    need: named,
                    outcome,
                });
            }
        }

        Ok(())
    }

    /// Each step's depth is one more than the deepest step it names.
    fn longest_chain(&self) -> usize {
        let mut depths = [0_usize; MAX_STEPS];
        let mut longest = 0;

        for index in 0..self.count.min(MAX_STEPS) {
            let names = self.steps.get(index).map_or(0, Step::names_mask);
            let mut deepest = 0;
            let mut bit = 1_u16;

            for depth in &depths {
                if names & bit != 0 {
                    deepest = deepest.max(*depth);
                }

                bit = bit.wrapping_shl(1);
            }

            let depth = deepest.saturating_add(1);
            longest = longest.max(depth);

            if let Some(slot) = depths.get_mut(index) {
                *slot = depth;
            }
        }

        longest
    }

    /// The steps a restart of step `index` moves, as a mask.
    pub(crate) fn reach(&self, index: usize) -> u16 {
        self.reach.get(index).copied().unwrap_or(0)
    }

    /// The steps that wait for step `index`'s final failure, as a mask.
    pub(crate) fn handlers(&self, index: usize) -> u16 {
        self.handlers.get(index).copied().unwrap_or(0)
    }

    pub(crate) fn listed(&self) -> &[Step] {
        self.steps.split_at(self.count.min(MAX_STEPS)).0
    }

    pub(crate) fn step(&self, index: usize) -> Option<&Step> {
        self.listed().get(index)
    }

    /// How many steps the plan has.
    #[must_use]
    pub const fn steps(&self) -> usize {
        self.count
    }

    /// The needs of step `index` that must all hold, one for each step they
    /// name, in order.
    #[must_use]
    pub fn after(&self, index: usize) -> Option<impl Iterator<Item = Need> + '_> {
        self.step(index).map(|step| step.after.needs())
    }

    /// The needs of step `index` of which one must hold, when there are any,
    /// one for each step they name, in order.
    #[must_use]
    pub fn after_any(&self, index: usize) -> Option<impl Iterator<Item = Need> + '_> {
        self.step(index).map(|step| step.after_any.needs())
    }

    /// How many outcomes step `index` has besides `Outcome::FAILED`.
    #[must_use]
    pub fn outcomes(&self, index: usize) -> Option<u8> {
        self.step(index).map(Step::outcome_count)
    }

    /// How many times step `index` may run before `Outcome::FAILED` is final.
    #[must_use]
    pub fn attempts(&self, index: usize) -> Option<u8> {
        self.step(index).map(Step::attempt_limit)
    }

    /// The longest chain of needs, counting its steps: the most `advance`
    /// passes in which a step can start, without a retry or a restart.
    #[must_use]
    pub const fn depth(&self) -> usize {
        self.depth
    }
}

/// Why a need no plan could hold is refused.
const fn fault(index: usize, need: Need) -> PlanError {
    let named = need.step();

    match need.stray() {
        _ if named >= index => PlanError::NotEarlier {
            step: index,
            need: named,
        },
        Some(outcome) => PlanError::NoSuchOutcome {
            step: index,
            need: named,
            outcome,
        },
        None => PlanError::NoOutcomes {
            step: index,
            need: named,
        },
    }
}
