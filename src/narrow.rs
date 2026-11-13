//! Outcome sets a byte wide, for Kani. The proofs range over steps with up
//! to three outcomes, so a set holds codes 0 to 2, `SKIPPED` and `FAILED`,
//! and every other code is past any set. The code that uses them is the
//! same at either width.

use crate::Outcome;
use crate::need::SKIPPED;

pub type Bits = u8;

pub fn bit(code: u8) -> Option<Bits> {
    let place = match code {
        0..=2 => code,
        SKIPPED => 6,
        127 => 7,
        _ => return None,
    };

    1_u8.checked_shl(u32::from(place))
}

pub fn below(count: u8) -> Bits {
    bit(count.min(3)).map_or(0b111, |bit| bit.wrapping_sub(1))
}

pub fn lowest(bits: Bits) -> Option<u8> {
    (0..=Outcome::FAILED.0).find(|code| bit(*code).is_some_and(|bit| bits & bit != 0))
}
