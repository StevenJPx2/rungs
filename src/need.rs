use core::fmt;

/// An outcome a step settles with: its own outcomes are numbered from 0, and
/// `Outcome::FAILED` is every step's final failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub struct Outcome(pub u8);

impl Outcome {
    /// A step's failure once its attempts are spent.
    pub const FAILED: Self = Self(127);

    /// Its bit in an `Outcomes` set; none past `FAILED`.
    fn bit(self) -> Option<Bits> {
        bit(self.0)
    }
}

/// A set of codes: each outcome, `SKIPPED` and `FAILED`, one bit each.
#[cfg(not(kani))]
pub type Bits = u128;

#[cfg(kani)]
pub use crate::narrow::{Bits, below, bit, lowest};

/// Bit `code` of a set: an outcome, `SKIPPED` or `FAILED`.
#[cfg(not(kani))]
pub fn bit(code: u8) -> Option<Bits> {
    1_u128.checked_shl(u32::from(code))
}

/// The codes below `count`.
#[cfg(not(kani))]
fn below(count: u8) -> Bits {
    bit(count).map_or(Bits::MAX, |bit| bit.wrapping_sub(1))
}

/// The lowest code in `bits`.
#[cfg(not(kani))]
fn lowest(bits: Bits) -> Option<u8> {
    u8::try_from(bits.trailing_zeros()).ok()
}

/// The code a skipped step matches in a need's set. No step has outcome
/// 126, so only `Need::settled` holds it.
pub const SKIPPED: u8 = 126;

/// A set of outcomes, as a need names them.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Outcomes(Bits);

impl Outcomes {
    #[must_use]
    pub fn contains(self, outcome: Outcome) -> bool {
        outcome.bit().is_some_and(|bit| self.0 & bit != 0)
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The outcomes in the set, lowest first.
    pub fn iter(self) -> impl Iterator<Item = Outcome> {
        (0..=Outcome::FAILED.0)
            .map(Outcome)
            .filter(move |outcome| self.contains(*outcome))
    }

    pub(crate) const fn from_bits(bits: Bits) -> Self {
        Self(bits)
    }

    /// The lowest outcome in the set that is neither below `count` nor
    /// `FAILED`.
    pub(crate) fn beyond(self, count: u8) -> Option<Outcome> {
        let failed = Outcome::FAILED.bit().unwrap_or(0);
        let rest = self.0 & !(below(count) | failed);

        if rest == 0 {
            return None;
        }

        lowest(rest).map(Outcome)
    }
}

impl fmt::Debug for Outcomes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

/// What a need wants of the step it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Wants {
    /// The step finished, with any outcome, or was skipped.
    Settled,
    /// The step settled with one of these outcomes.
    Outcome(Outcomes),
}

/// A step's need on an earlier step.
///
/// ```
/// use rungs::{Need, Outcome, Wants};
///
/// let need = Need::outcome(0, [Outcome(1), Outcome::FAILED]);
///
/// assert_eq!(need.step(), 0);
/// assert!(matches!(need.wants(), Wants::Outcome(set) if set.contains(Outcome::FAILED)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Need {
    step: usize,
    wants: Wants,
    /// An outcome too large for any set, kept so `Plan::new` can refuse it.
    stray: Option<Outcome>,
}

impl Need {
    /// Step `step` has finished, with any outcome, or was skipped.
    #[must_use]
    pub const fn settled(step: usize) -> Self {
        Self {
            step,
            wants: Wants::Settled,
            stray: None,
        }
    }

    /// Step `step` settled with an outcome in `outcomes`. Never matches a
    /// skipped step; `FAILED` matches only when it is in the set.
    #[must_use]
    pub fn outcome(step: usize, outcomes: impl IntoIterator<Item = Outcome>) -> Self {
        let mut set = 0;
        let mut stray = None;

        for outcome in outcomes {
            match outcome.bit() {
                Some(bit) => set |= bit,
                None => stray = stray.or(Some(outcome)),
            }
        }

        Self {
            step,
            wants: Wants::Outcome(Outcomes(set)),
            stray,
        }
    }

    /// The step it names.
    #[must_use]
    pub const fn step(&self) -> usize {
        self.step
    }

    #[must_use]
    pub const fn wants(&self) -> Wants {
        self.wants
    }

    /// A need read back from a plan, which holds only well-formed ones.
    pub(crate) const fn kept(step: usize, accepts: Bits) -> Self {
        let wants = if accepts == Bits::MAX {
            Wants::Settled
        } else {
            Wants::Outcome(Outcomes(accepts))
        };

        Self {
            step,
            wants,
            stray: None,
        }
    }

    /// The codes it holds for: every outcome and `SKIPPED` for a settled
    /// need, its set for an outcome need.
    pub(crate) const fn accepts(&self) -> Bits {
        match self.wants {
            Wants::Settled => Bits::MAX,
            Wants::Outcome(set) => set.0,
        }
    }

    pub(crate) const fn stray(&self) -> Option<Outcome> {
        self.stray
    }
}
