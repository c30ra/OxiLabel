//! Geometry of Blender-style pie menus: up to eight buttons at compass positions around the point
//! where the menu opened. The direction from that centre to the pointer picks the button.

use crate::geometry::Point;

/// Pointer offsets shorter than this (screen pixels) point at nothing.
pub const DEAD_ZONE: f64 = 18.0;
/// While the pie key is held, moving this far picks the highlighted button at once ("flick").
pub const FLICK_DISTANCE: f64 = 150.0;
/// A press and release of the pie key shorter than this (milliseconds) is a tap.
pub const TAP_MILLIS: f64 = 220.0;

/// The eight pie positions; screen y grows downwards, so `N` is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Compass {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

impl Compass {
    pub const ALL: [Compass; 8] =
        [Compass::N, Compass::NE, Compass::E, Compass::SE, Compass::S, Compass::SW, Compass::W, Compass::NW];

    /// Direction in radians, measured like `atan2(dy, dx)` in screen coordinates.
    pub fn angle(self) -> f64 {
        let eighths = match self {
            Compass::E => 0.0,
            Compass::SE => 1.0,
            Compass::S => 2.0,
            Compass::SW => 3.0,
            Compass::W => 4.0,
            Compass::NW => -3.0,
            Compass::N => -2.0,
            Compass::NE => -1.0,
        };
        eighths * std::f64::consts::FRAC_PI_4
    }

    /// Where the centre of this position's button sits relative to the pie centre, in screen
    /// pixels; `wide` is for pies of four wider buttons.
    pub fn offset(self, wide: bool) -> Point {
        // Matches the approved design: buttons on an ellipse, corners pulled in.
        let (x, y) = match self {
            Compass::N => (0.0, -128.0),
            Compass::S => (0.0, 128.0),
            Compass::E => (if wide { 113.0 } else { 129.0 }, 0.0),
            Compass::W => (if wide { -113.0 } else { -129.0 }, 0.0),
            Compass::NE => (97.0, -84.0),
            Compass::SE => (97.0, 84.0),
            Compass::SW => (-97.0, 84.0),
            Compass::NW => (-97.0, -84.0),
        };
        Point::new(x, y)
    }
}

/// The slot among `slots` whose direction is closest to the pointer offset (`dx`, `dy`), or
/// `None` inside the dead zone or when there are no slots.
pub fn pick(dx: f64, dy: f64, slots: &[Compass]) -> Option<Compass> {
    if dx.hypot(dy) < DEAD_ZONE {
        return None;
    }
    let pointer = dy.atan2(dx);
    let gap = |slot: &Compass| {
        let difference = (pointer - slot.angle()).rem_euclid(std::f64::consts::TAU);
        difference.min(std::f64::consts::TAU - difference)
    };
    slots.iter().copied().min_by(|a, b| gap(a).total_cmp(&gap(b)))
}

/// Whether a press of the pie key counts as a tap: short and without pointing anywhere.
pub fn is_tap(held_millis: f64, dx: f64, dy: f64) -> bool {
    held_millis < TAP_MILLIS && dx.hypot(dy) < DEAD_ZONE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dead_zone_points_at_nothing() {
        assert_eq!(pick(5.0, -5.0, &Compass::ALL), None);
    }

    #[test]
    fn each_direction_picks_its_compass_position() {
        assert_eq!(pick(0.0, -60.0, &Compass::ALL), Some(Compass::N));
        assert_eq!(pick(60.0, 0.0, &Compass::ALL), Some(Compass::E));
        assert_eq!(pick(0.0, 60.0, &Compass::ALL), Some(Compass::S));
        assert_eq!(pick(-60.0, 0.0, &Compass::ALL), Some(Compass::W));
        assert_eq!(pick(40.0, -40.0, &Compass::ALL), Some(Compass::NE));
        assert_eq!(pick(-40.0, 40.0, &Compass::ALL), Some(Compass::SW));
        // Each position owns a 45° sector: 20° off north is still north, 25° is north-east.
        let (sin20, cos20) = 20f64.to_radians().sin_cos();
        assert_eq!(pick(60.0 * sin20, -60.0 * cos20, &Compass::ALL), Some(Compass::N));
        let (sin25, cos25) = 25f64.to_radians().sin_cos();
        assert_eq!(pick(60.0 * sin25, -60.0 * cos25, &Compass::ALL), Some(Compass::NE));
    }

    #[test]
    fn a_four_slot_pie_snaps_to_the_nearest_available_slot() {
        let slots = [Compass::N, Compass::E, Compass::S, Compass::W];
        assert_eq!(pick(40.0, -35.0, &slots), Some(Compass::E));
        assert_eq!(pick(35.0, -40.0, &slots), Some(Compass::N));
        assert_eq!(pick(-1.0, 60.0, &slots), Some(Compass::S));
        assert_eq!(pick(60.0, 0.0, &[]), None);
    }

    #[test]
    fn button_positions_match_their_directions() {
        for slot in Compass::ALL {
            let at = slot.offset(false);
            assert_eq!(pick(at.x, at.y, &Compass::ALL), Some(slot), "{slot:?}");
        }
    }

    #[test]
    fn only_a_short_press_without_pointing_is_a_tap() {
        assert!(is_tap(120.0, 3.0, 2.0));
        assert!(!is_tap(400.0, 3.0, 2.0));
        assert!(!is_tap(120.0, 40.0, 0.0));
    }
}
