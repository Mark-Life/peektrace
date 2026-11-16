//! The first pass: a skeleton shaped like the report, why it takes a
//! while, and the seconds since this scope's pass started.

use rune::{El, col, row, text};

use crate::state::Ui;
use crate::theme::{Theme, ty};
use crate::views::widgets::{skeleton, status};

fn table_block(t: &Theme) -> El {
    col()
        .w_full()
        .gap(12.0)
        .child(skeleton(Some(160.0), 20.0, t))
        .child(skeleton(None, 112.0, t))
}

pub fn pending(s: &Ui, t: &Theme) -> El {
    col()
        .key("stats-pending")
        .w_full()
        .gap(32.0)
        .child(
            col()
                .w_full()
                .gap(8.0)
                .child(
                    row()
                        .w_full()
                        .flex_wrap()
                        .gap(12.0)
                        .justify_between()
                        .center_y()
                        .child(skeleton(None, 20.0, t).max_w(448.0).grow(1.0))
                        .child(skeleton(Some(288.0), 28.0, t)),
                )
                .child(
                    row()
                        .w_full()
                        .gap(8.0)
                        .items_end()
                        .child(status(
                            "Reading transcripts. The first pass parses every file and can take about 20 s; later passes read the cache.",
                            t,
                        ))
                        .child(
                            text(format!("{} s", peektrace_js::num::to_js_string(s.stats.elapsed())))
                                .font(ty::MONO_XS)
                                .color(t.muted_fg)
                                .key("stats-elapsed"),
                        ),
                ),
        )
        .child(
            col()
                .w_full()
                .gap(12.0)
                .child(skeleton(Some(192.0), 20.0, t))
                .child(skeleton(None, 80.0, t)),
        )
        .child(table_block(t))
        .child(table_block(t))
        .child(table_block(t))
}
