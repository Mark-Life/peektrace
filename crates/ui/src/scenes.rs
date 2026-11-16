//! Named starting states for headless goldens.

use peektrace_protocol::AgentId;

use crate::shell::load_now;
use crate::state::settings::FormRow;
use crate::state::{Route, Ui};
use crate::state_memory;

/// Scenes for headless goldens. Detail scenes open the newest session, or
/// the one an extra `session=<id>` argument names.
/// A named starting state.
pub type Scene = (&'static str, fn(&mut Ui));

pub const SCENES: &[Scene] = &[
    ("list", |_| {}),
    ("list-filtered", |s| {
        s.query = rune::TextEdit::new("refactor");
        s.apply_query();
    }),
    ("detail-top", open_scene),
    ("detail-pending", |s| {
        if let Some(id) = scene_session(s) {
            s.select(&id, None);
        }
    }),
    ("detail-chart-hover", |s| {
        open_scene(s);
        s.hover = s
            .detail
            .as_ref()
            .and_then(|d| d.a.snapshots.len().checked_sub(1))
            .map(|n| n / 2);
    }),
    ("history-open", |s| {
        open_scene(s);
        s.expand_all();
    }),
    ("light", |s| {
        open_scene(s);
        s.dark = Some(false);
    }),
    ("nav-collapsed", |s| s.nav_collapsed = true),
    ("stats", stats_scene),
    ("stats-pending", |s| s.enter_stats(0.0)),
    ("stats-cause-open", |s| {
        stats_scene(s);
        s.stats.open_cause = first_cause(s);
    }),
    ("stats-drill", |s| {
        stats_scene(s);
        if let Some(key) = first_cause(s) {
            s.open_drill(peektrace_protocol::DrillTable::Cause, &key);
            load_now(s);
        }
    }),
    ("stats-findings-all", |s| {
        stats_scene(s);
        s.stats.show_all = true;
    }),
    ("stats-light", |s| {
        stats_scene(s);
        s.dark = Some(false);
    }),
    ("settings", settings_scene),
    ("settings-rows", |s| {
        settings_scene(s);
        s.settings.rows[0] = vec![
            FormRow {
                path: rune::TextEdit::new("~/work/.claude"),
                label: rune::TextEdit::new("work"),
            },
            FormRow::default(),
        ];
    }),
    ("settings-conflict", |s| {
        settings_scene(s);
        s.settings.conflict = true;
    }),
    ("settings-readonly", |s| {
        settings_scene(s);
        if let Some(v) = &mut s.info.value {
            v.read_only = true;
        }
    }),
    ("capabilities", capabilities_scene),
    ("capabilities-cell", |s| {
        capabilities_scene(s);
        s.open_cell("session.debug-context", AgentId::Codex);
    }),
    ("shortcuts", |s| {
        s.help_open = true;
        s.mac = true;
    }),
    ("memory", state_memory::scene),
    ("memory-pending", |s| s.go(Route::Memory)),
    ("memory-filtered", state_memory::scene_filtered),
    ("memory-entry", state_memory::scene_entry),
    ("memory-create", state_memory::scene_create),
];

fn stats_scene(s: &mut Ui) {
    s.enter_stats(0.0);
    load_now(s);
}

fn first_cause(s: &Ui) -> Option<String> {
    s.stats
        .screen()
        .and_then(|x| x.report.causes.first())
        .map(|c| c.key.clone())
}

fn settings_scene(s: &mut Ui) {
    s.go(Route::Settings);
    load_now(s);
}

fn capabilities_scene(s: &mut Ui) {
    s.go(Route::Capabilities);
    load_now(s);
}

fn scene_session(s: &Ui) -> Option<String> {
    rune::extra_args()
        .iter()
        .find_map(|a| a.strip_prefix("session=").map(str::to_owned))
        .or_else(|| {
            s.rail
                .rows
                .first()
                .and_then(|&i| s.rail.header(i))
                .map(|h| h.id.clone())
        })
}

fn open_scene(s: &mut Ui) {
    if let Some(id) = scene_session(s) {
        s.select(&id, None);
        load_now(s);
    }
}
