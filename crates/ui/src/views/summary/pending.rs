//! The session detail while its analysis loads: a skeleton shaped like the
//! verdict card, the growth chart and the first history rows.

use rune::{El, col, row};

use super::tile_columns;
use crate::theme::Theme;
use crate::views::widgets::{card, grid, skeleton, status};

/// Tiles a loaded summary shows without a cost cell.
const TILES: usize = 6;

fn verdict(width: f32, t: &Theme) -> El {
    // Blocks cap at their loaded size; a narrow column wraps the badges.
    let header = row()
        .w_full()
        .flex_wrap()
        .gap(8.0)
        .items_start()
        .child(
            col()
                .grow(1.0)
                .shrink()
                .min_w(120.0)
                .gap(8.0)
                .child(skeleton(None, 24.0, t).max_w(240.0))
                .child(skeleton(None, 14.0, t).max_w(360.0)),
        )
        .child(skeleton(Some(88.0), 18.0, t))
        .child(skeleton(Some(72.0), 18.0, t));
    let tiles = (0..TILES).map(|_| skeleton(None, 51.0, t));
    card(t)
        .child(header)
        .child(skeleton(None, 16.0, t))
        .child(grid(tile_columns(width), 8.0, tiles))
        .child(
            row()
                .w_full()
                .flex_wrap()
                .gap(6.0)
                .children((0..3).map(|_| skeleton(Some(128.0), 24.0, t))),
        )
}

/// The skeleton for a detail column `width` wide.
pub fn pending(width: f32, t: &Theme) -> El {
    col()
        .key("detail-pending")
        .w_full()
        .gap(16.0)
        .child(status("Loading session…", t))
        .child(verdict(width, t))
        .child(
            card(t)
                .child(skeleton(None, 20.0, t).max_w(192.0))
                .child(skeleton(None, 320.0, t)),
        )
        .child(skeleton(None, 20.0, t).max_w(160.0))
        .children((0..3).map(|_| skeleton(None, 56.0, t)))
}
