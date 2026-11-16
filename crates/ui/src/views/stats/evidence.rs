//! Secondary evidence: the waiting panel and the error clusters, each a
//! ranked list of proportional bars.

use peektrace_protocol::DrillTable;
use peektrace_protocol::stats::{ClusterRowView, WaitPanelView};
use rune::{Color, El, col, el, row, spacer, text};

use super::{muted, sky};
use crate::model::stats::rows::{
    RankRow, cluster_max, cluster_rows, more_clusters, shape_rows, wait_figures, wait_note,
    width_of,
};
use crate::state::Ui;
use crate::theme::{Theme, ty};

#[expect(clippy::cast_possible_truncation, reason = "a percentage")]
fn bar(pct: f64, t: &Theme) -> El {
    let frac = (pct / 100.0).clamp(0.0, 1.0) as f32;
    row().w_full().h(6.0).radius(3.0).fill(t.muted).child(
        el().w_frac(frac)
            .h_full()
            .radius(3.0)
            .fill(sky().alpha(0.7)),
    )
}

fn rank_body(r: &RankRow, max: f64, t: &Theme) -> El {
    col()
        .w_full()
        .gap(4.0)
        .child(
            row()
                .w_full()
                .gap(12.0)
                .items_end()
                .child(
                    text(r.label.clone())
                        .font(ty::XS_MEDIUM)
                        .color(t.fg)
                        .truncate()
                        .min_w(0.0),
                )
                .child(spacer())
                .child(
                    text(r.value_text.clone())
                        .font(ty::MONO_XS)
                        .color(t.muted_fg),
                )
                .child(
                    text(r.secondary.clone())
                        .font(ty::MONO_XS)
                        .color(t.muted_fg.alpha(0.7)),
                ),
        )
        .child(bar(width_of(r.value, max), t))
}

/// Ranked bars in the order they arrive; `pick` makes rows open a drill.
fn rank_bar(
    section: &str,
    rows: &[RankRow],
    max: f64,
    pick: Option<(DrillTable, Option<&str>)>,
    t: &Theme,
) -> El {
    if rows.is_empty() {
        return text("Nothing measured here.")
            .font(ty::SM)
            .color(t.muted_fg)
            .key("rank-empty");
    }
    col()
        .key("rank-bar")
        .w_full()
        .gap(8.0)
        .children(rows.iter().map(|r| {
            let body = rank_body(r, max, t);
            let key = format!("rank-row:{section}:{}", r.key);
            match pick {
                None => body.key(key),
                Some((table, selected)) => {
                    let on = selected == Some(r.key.as_str());
                    let row_key = r.key.clone();
                    t.kit
                        .interactive(body.px(8.0).py(4.0).radius(6.0).fill(if on {
                            t.muted.alpha(0.6)
                        } else {
                            Color::TRANSPARENT
                        }))
                        .key(key)
                        .role("button")
                        .label(r.label.clone())
                        .focusable(true)
                        .on_press(move |s: &mut Ui| s.open_drill(table, &row_key))
                }
            }
        }))
}

pub fn waiting(w: &WaitPanelView, t: &Theme) -> El {
    col()
        .key("stats-waiting")
        .w_full()
        .gap(12.0)
        .child(
            row()
                .flex_wrap()
                .gap(24.0)
                .children(wait_figures(w).into_iter().map(|(value, label)| {
                    col()
                        .child(text(value).font(ty::MONO_SM).color(t.fg))
                        .child(text(label).font(ty::XS).color(t.muted_fg))
                })),
        )
        .child(rank_bar("wait", &shape_rows(w), w.total_sec, None, t))
        .children(wait_note(w).map(|n| muted(n, t)))
}

pub fn clusters(s: &Ui, rows: &[ClusterRowView], t: &Theme) -> El {
    let selected = s
        .stats
        .drill
        .as_ref()
        .filter(|(tb, _)| *tb == DrillTable::Cluster)
        .map(|(_, k)| k.as_str());
    col()
        .key("stats-clusters")
        .w_full()
        .gap(12.0)
        .child(rank_bar(
            "cluster",
            &cluster_rows(rows),
            cluster_max(rows),
            Some((DrillTable::Cluster, selected)),
            t,
        ))
        .children(more_clusters(rows).map(|m| muted(m, t)))
}
