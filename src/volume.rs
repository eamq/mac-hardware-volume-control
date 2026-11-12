//! Discrete 64-step volume grid with 1:1 linear mapping.
//!
//! - Grid range: `0..=64` (65 discrete states, 64 audible/step increments).
//! - Standard F11 / F12 press: moves to the next/previous 1/16th tick (multiple of 4;
//!   snaps to the nearest 1/16th in the press direction when on an intermediate tick).
//! - `Option + Shift + F11 / F12` micro-step: `±1` step (64 fine steps).
//! - Linear 1:1 mapping (`0%` = Mute, `25%` = step 16, `50%` = step 32, `100%` = step 64)
//!   so the macOS HUD matches the physical LED ring on the audio interface.

use std::fmt;

/// Maximum step index on the discrete volume grid.
pub const MAX_STEP: u8 = 64;

/// Step delta for standard volume up/down key presses (`F11` / `F12`).
pub const STANDARD_STEP_DELTA: u8 = 4;

/// Step delta for fine-grained micro-step presses (`Option + Shift + F11/F12`).
pub const MICRO_STEP_DELTA: u8 = 1;

/// Direction and granularity of a volume key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeAction {
    Up { fine: bool },
    Down { fine: bool },
    ToggleMute,
}

/// Immutable snapshot of the 64-step volume state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeState {
    step: u8,
    muted: bool,
    /// Step remembered before muting so unmuting restores the previous level.
    pre_mute_step: u8,
}

impl Default for VolumeState {
    fn default() -> Self {
        Self::from_step(32, false)
    }
}

impl VolumeState {
    /// Creates a new state clamped to `0..=64`.
    /// If `step == 0`, `muted` is forced to `true`.
    #[must_use]
    pub fn from_step(step: u8, muted: bool) -> Self {
        let clamped = step.min(MAX_STEP);
        let effective_muted = muted || clamped == 0;
        Self {
            step: clamped,
            muted: effective_muted,
            pre_mute_step: if clamped > 0 {
                clamped
            } else {
                STANDARD_STEP_DELTA
            },
        }
    }

    /// Creates a state from a normalized linear ratio `0.0..=1.0`, rounding to the nearest step.
    #[must_use]
    pub fn from_linear(ratio: f32, muted: bool) -> Self {
        let clamped = ratio.clamp(0.0, 1.0);
        let step = (clamped * f32::from(MAX_STEP)).round() as u8;
        Self::from_step(step, muted)
    }

    /// Returns the current discrete step (`0..=64`).
    #[must_use]
    pub const fn step(self) -> u8 {
        self.step
    }

    /// Returns whether output is currently muted.
    #[must_use]
    pub const fn is_muted(self) -> bool {
        self.muted
    }

    /// Returns the effective step (`0` when muted, otherwise `self.step`).
    #[must_use]
    pub const fn effective_step(self) -> u8 {
        if self.muted {
            0
        } else {
            self.step
        }
    }

    /// Returns the 1:1 linear normalized volume in `0.0..=1.0` (`0.0` when muted).
    #[must_use]
    pub fn as_linear(self) -> f32 {
        f32::from(self.effective_step()) / f32::from(MAX_STEP)
    }

    /// Returns the 1:1 linear percentage in `0.0..=100.0` (`0.0` when muted).
    #[must_use]
    pub fn as_percent(self) -> f32 {
        self.as_linear() * 100.0
    }

    /// Applies a volume action and returns the updated state.
    ///
    /// Standard presses (`fine: false`) snap to the next or previous 1/16th tick
    /// (multiple of 4) when starting from an intermediate micro-step, while
    /// micro-steps (`fine: true`) always move by `±1`.
    #[must_use]
    pub fn apply(self, action: VolumeAction) -> Self {
        match action {
            VolumeAction::Up { fine } => {
                let base = if self.muted && self.step == 0 {
                    0
                } else {
                    self.step
                };
                let next = if fine {
                    base.saturating_add(MICRO_STEP_DELTA).min(MAX_STEP)
                } else {
                    // Snap up to the next 1/16th (multiple of 4) when on an intermediate tick.
                    ((base / STANDARD_STEP_DELTA) + 1)
                        .saturating_mul(STANDARD_STEP_DELTA)
                        .min(MAX_STEP)
                };
                Self {
                    step: next,
                    muted: false,
                    pre_mute_step: next,
                }
            }
            VolumeAction::Down { fine } => {
                let next = if fine {
                    self.step.saturating_sub(MICRO_STEP_DELTA)
                } else if self.step == 0 {
                    0
                } else {
                    // Snap down to the previous 1/16th (multiple of 4) when on an intermediate tick.
                    ((self.step - 1) / STANDARD_STEP_DELTA) * STANDARD_STEP_DELTA
                };
                if next == 0 {
                    Self {
                        step: 0,
                        muted: true,
                        pre_mute_step: self.pre_mute_step,
                    }
                } else {
                    Self {
                        step: next,
                        muted: false,
                        pre_mute_step: next,
                    }
                }
            }
            VolumeAction::ToggleMute => {
                if self.muted {
                    let restored = self.pre_mute_step.clamp(1, MAX_STEP);
                    Self {
                        step: restored,
                        muted: false,
                        pre_mute_step: restored,
                    }
                } else {
                    Self {
                        step: self.step,
                        muted: true,
                        pre_mute_step: if self.step > 0 {
                            self.step
                        } else {
                            self.pre_mute_step
                        },
                    }
                }
            }
        }
    }
}

impl fmt::Display for VolumeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.muted {
            write!(f, "MUTED (0/{MAX_STEP}, 0%)")
        } else {
            write!(f, "{}/{} ({:.0}%)", self.step, MAX_STEP, self.as_percent())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_percentages_match_1_to_1_grid() {
        assert_eq!(VolumeState::from_step(0, false).as_percent(), 0.0);
        assert!(VolumeState::from_step(0, false).is_muted());

        assert_eq!(VolumeState::from_step(16, false).as_percent(), 25.0);
        assert_eq!(VolumeState::from_step(32, false).as_percent(), 50.0);
        assert_eq!(VolumeState::from_step(48, false).as_percent(), 75.0);
        assert_eq!(VolumeState::from_step(64, false).as_percent(), 100.0);
    }

    #[test]
    fn standard_step_is_four_and_micro_step_is_one() {
        let s = VolumeState::from_step(32, false);
        assert_eq!(s.apply(VolumeAction::Up { fine: false }).step(), 36);
        assert_eq!(s.apply(VolumeAction::Down { fine: false }).step(), 28);
        assert_eq!(s.apply(VolumeAction::Up { fine: true }).step(), 33);
        assert_eq!(s.apply(VolumeAction::Down { fine: true }).step(), 31);
    }

    #[test]
    fn clamping_at_bounds() {
        let top = VolumeState::from_step(62, false).apply(VolumeAction::Up { fine: false });
        assert_eq!(top.step(), 64);
        assert!(!top.is_muted());

        let bottom = VolumeState::from_step(2, false).apply(VolumeAction::Down { fine: false });
        assert_eq!(bottom.step(), 0);
        assert!(bottom.is_muted());
        assert_eq!(bottom.as_linear(), 0.0);
    }

    #[test]
    fn toggle_mute_preserves_and_restores_step() {
        let s = VolumeState::from_step(24, false);
        let muted = s.apply(VolumeAction::ToggleMute);
        assert!(muted.is_muted());
        assert_eq!(muted.effective_step(), 0);
        assert_eq!(muted.as_linear(), 0.0);

        let unmuted = muted.apply(VolumeAction::ToggleMute);
        assert!(!unmuted.is_muted());
        assert_eq!(unmuted.step(), 24);
        assert_eq!(unmuted.effective_step(), 24);
    }

    #[test]
    fn standard_press_snaps_from_intermediate_ticks_to_nearest_16th() {
        // Off-grid step 33 sits between 32 (8/16) and 36 (9/16)
        let s = VolumeState::from_step(33, false);
        assert_eq!(s.apply(VolumeAction::Up { fine: false }).step(), 36);
        assert_eq!(s.apply(VolumeAction::Down { fine: false }).step(), 32);

        // Off-grid step 35 also snaps to 36 (up) and 32 (down)
        let s2 = VolumeState::from_step(35, false);
        assert_eq!(s2.apply(VolumeAction::Up { fine: false }).step(), 36);
        assert_eq!(s2.apply(VolumeAction::Down { fine: false }).step(), 32);

        // Near bottom (step 3) snaps down to 0 (Mute) and up to 4
        let low = VolumeState::from_step(3, false);
        let low_down = low.apply(VolumeAction::Down { fine: false });
        assert_eq!(low_down.step(), 0);
        assert!(low_down.is_muted());
        assert_eq!(low.apply(VolumeAction::Up { fine: false }).step(), 4);
    }

    #[test]
    fn from_linear_rounds_accurately() {
        assert_eq!(VolumeState::from_linear(0.25, false).step(), 16);
        assert_eq!(VolumeState::from_linear(0.50, false).step(), 32);
        assert_eq!(VolumeState::from_linear(1.00, false).step(), 64);
        assert_eq!(VolumeState::from_linear(0.00, false).step(), 0);
    }
}
