//! Which snapshot a pointer over the growth chart points at.

use crate::model::jsnum::js_round;

/// Nearest snapshot to viewBox x `px`: snapshots sit evenly from the plot's
/// left edge to its right one (one snapshot sits in the middle). `None`
/// when there are none.
pub fn snap_index_at_x(left: f64, n: usize, plot_w: f64, px: f64) -> Option<usize> {
    if n == 0 {
        return None;
    }
    if n == 1 || plot_w <= 0.0 {
        return Some(0);
    }
    let frac = ((px - left) / plot_w).clamp(0.0, 1.0);
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a small index"
    )]
    let i = js_round(frac * (n - 1) as f64) as usize;
    Some(i)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickAction {
    Show,
    Jump,
}

/// A mouse click jumps at once; a touch shows the guide first and jumps on
/// a second tap of the same snapshot (`armed`).
pub fn click_action(armed: Option<usize>, i: usize, touch: bool) -> ClickAction {
    if touch && armed != Some(i) {
        ClickAction::Show
    } else {
        ClickAction::Jump
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_snapshot() {
        assert_eq!(snap_index_at_x(76.0, 0, 906.0, 500.0), None);
        assert_eq!(snap_index_at_x(76.0, 1, 906.0, 0.0), Some(0));
        assert_eq!(snap_index_at_x(76.0, 5, 0.0, 300.0), Some(0));
        assert_eq!(snap_index_at_x(76.0, 11, 906.0, 76.0), Some(0));
        assert_eq!(snap_index_at_x(76.0, 11, 906.0, 982.0), Some(10));
        assert_eq!(snap_index_at_x(76.0, 11, 906.0, 2000.0), Some(10));
        assert_eq!(snap_index_at_x(76.0, 11, 906.0, -50.0), Some(0));
        // Halfway between 4 and 5 rounds up, as Math.round does.
        assert_eq!(snap_index_at_x(0.0, 11, 100.0, 45.0), Some(5));
    }

    #[test]
    fn touch_needs_two_taps() {
        assert_eq!(click_action(None, 3, false), ClickAction::Jump);
        assert_eq!(click_action(None, 3, true), ClickAction::Show);
        assert_eq!(click_action(Some(3), 3, true), ClickAction::Jump);
        assert_eq!(click_action(Some(2), 3, true), ClickAction::Show);
    }
}
