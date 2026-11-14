//! Settings: extra agent roots in `settings.json`, saved with a
//! compare-and-swap on the file's mtime. Element keys follow the
//! inspector's `data-testid`s.

use std::time::Duration;

use peektrace_protocol::{AgentId, ScannedRoot, ServerInfo, SettingsResult};
use rune::{El, ScrollSpec, TextStyle, Tx, col, icon, row, spacer, text};
use rune_kit::{
    Button, ButtonVariant, ControlSize, Field, IconButton, IconButtonVariant, TextInput, glyphs,
};

use super::blocks::{alert, failure, pending, rich};
use super::widgets::card;
use crate::model::list::agent_label;
use crate::model::settings::{
    AGENT_ORDER, NATIVE, copy, default_roots, read_only_body, restart_body, scans, tilde_path,
};
use crate::state::settings::slot;
use crate::state::{Route, Ui};
use crate::theme::{Theme, hex_or_grey, ty};

/// The inspector's `max-w-3xl` column.
const COLUMN_W: f32 = 768.0;
const CARD_TITLE: TextStyle = TextStyle::new(14.0, 600.0);
const AMBER_500: &str = "#f59e0b";
const AMBER_300: &str = "#fcd34d";

fn titled_card(title: &str, note: &str, t: &Theme) -> El {
    card(t)
        .child(text(title.to_owned()).font(CARD_TITLE).color(t.fg))
        .child(
            text(note.to_owned())
                .font(ty::SM)
                .color(t.muted_fg)
                .wrap()
                .w_full(),
        )
}

fn default_row(root: &ScannedRoot, home: &str, t: &Theme) -> El {
    let mut r = row()
        .key("settings-default-root")
        .w_full()
        .gap(8.0)
        .flex_wrap()
        .center_y()
        .child(text(copy::DEFAULT_LABEL).font(ty::SM_MEDIUM).color(t.fg))
        .child(
            text(tilde_path(&root.home, home))
                .font(ty::MONO_XS)
                .color(t.muted_fg),
        )
        .child(rich(
            &scans(&tilde_path(&root.path, home)),
            ty::XS,
            t.muted_fg,
            t,
        ));
    if !root.exists {
        r = r.child(super::widgets::badge(copy::NOT_FOUND, t.muted_fg, t).key("root-missing"));
    }
    r
}

fn root_row(s: &Ui, agent: AgentId, i: usize, off: bool, t: &Theme) -> El {
    let a = slot(agent);
    let id = agent.as_str();
    let Some(r) = s.settings.rows[a].get(i) else {
        return row();
    };
    let path = TextInput::new(&r.path)
        .placeholder(copy::PATH_PLACEHOLDER)
        .label(copy::PATH_LABEL)
        .disabled(off)
        .on_edit(move |s: &mut Ui| s.settings.edit(a, i, false))
        .build(&t.kit)
        .key(format!("settings-path-{id}-{i}"));
    let label = TextInput::new(&r.label)
        .placeholder(copy::LABEL_PLACEHOLDER)
        .label(copy::LABEL_LABEL)
        .disabled(off)
        .on_edit(move |s: &mut Ui| s.settings.edit(a, i, true))
        .build(&t.kit)
        .key(format!("settings-label-{id}-{i}"));
    let remove = IconButton::new(&glyphs::CLOSE)
        .variant(IconButtonVariant::Ghost)
        .label(copy::REMOVE_ROOT)
        .disabled(off)
        .on_press(move |s: &mut Ui| s.remove_root(agent, i))
        .build(&t.kit)
        .key(format!("settings-remove-{id}-{i}"));
    row()
        .w_full()
        .gap(8.0)
        .items_end()
        .child(
            Field::new()
                .label(copy::PATH_LABEL)
                .control(path)
                .build(&t.kit)
                .grow(1.0)
                .min_w(0.0),
        )
        .child(
            Field::new()
                .label(copy::LABEL_LABEL)
                .control(label)
                .build(&t.kit)
                .w(160.0),
        )
        .child(remove)
}

fn agent_card(
    s: &Ui,
    agent: AgentId,
    default: Option<&ScannedRoot>,
    home: &str,
    off: bool,
    t: &Theme,
) -> El {
    let mut c = titled_card(agent_label(agent), copy::AGENT_NOTE, t)
        .key(format!("settings-agent-{}", agent.as_str()));
    if let Some(root) = default {
        c = c.child(default_row(root, home, t));
    }
    let rows = &s.settings.rows[slot(agent)];
    if rows.is_empty() {
        c = c.child(text(copy::NO_ROOTS).font(ty::SM).color(t.muted_fg));
    }
    c = c.children((0..rows.len()).map(|i| root_row(s, agent, i, off, t)));
    c.child(
        row().child(
            Button::new(copy::ADD_ROOT)
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Small)
                .disabled(off)
                .on_press(move |s: &mut Ui| s.add_root(agent))
                .build(&t.kit)
                .key(format!("settings-add-{}", agent.as_str())),
        ),
    )
}

fn conflict(s: &Ui, off: bool, t: &Theme) -> El {
    let amber = hex_or_grey(AMBER_500);
    col()
        .key("settings-cas-conflict")
        .w_full()
        .gap(12.0)
        .padding(16.0)
        .radius(6.0)
        .border(1.0, amber.alpha(0.4))
        .fill(amber.alpha(0.1))
        .child(
            text(copy::CONFLICT_TITLE)
                .font(ty::SM_MEDIUM)
                .color(hex_or_grey(AMBER_300)),
        )
        .child(
            text(copy::CONFLICT_BODY)
                .font(ty::SM)
                .color(t.muted_fg)
                .wrap()
                .w_full(),
        )
        .child(
            row()
                .w_full()
                .gap(8.0)
                .child(spacer())
                .child(
                    Button::new(copy::CONFLICT_RELOAD)
                        .variant(ButtonVariant::Outline)
                        .disabled(off)
                        .on_press_tx(|s: &mut Ui, tx: &mut Tx| {
                            s.request_settings();
                            tx.wake_after(Duration::ZERO);
                        })
                        .build(&t.kit)
                        .key("settings-cas-reload"),
                )
                .child(
                    Button::new(copy::CONFLICT_OVERWRITE)
                        .variant(ButtonVariant::Danger)
                        .disabled(off || s.settings.busy)
                        .on_press_tx(|s: &mut Ui, tx: &mut Tx| {
                            s.save_settings(false);
                            tx.wake_after(Duration::ZERO);
                        })
                        .build(&t.kit)
                        .key("settings-cas-overwrite"),
                ),
        )
}

fn editor(s: &Ui, loaded: &SettingsResult, info: &ServerInfo, t: &Theme) -> El {
    let home = info.user_home.as_str();
    let off = info.read_only;
    let defaults = default_roots(info);
    let mut c = col().key("settings-editor").w_full().gap(16.0).child(alert(
        copy::RESTART_TITLE,
        rich(
            &restart_body(&tilde_path(&loaded.path, home), NATIVE),
            ty::SM,
            t.muted_fg,
            t,
        )
        .w_full(),
        None,
        false,
        t,
    ));
    if off {
        c = c.child(
            alert(
                copy::READ_ONLY_TITLE,
                rich(&read_only_body(), ty::SM, t.muted_fg, t).w_full(),
                Some(icon(&glyphs::LOCK, 16.0).color(t.fg).a11y_hidden()),
                false,
                t,
            )
            .key("settings-readonly"),
        );
    }
    let mut form = col().key("settings-form").w_full().gap(16.0).disabled(off);
    if s.settings.conflict {
        form = form.child(conflict(s, off, t));
    }
    form = form.children(AGENT_ORDER.into_iter().map(|agent| {
        let default = defaults.iter().find(|(a, _)| *a == agent).map(|(_, r)| *r);
        agent_card(s, agent, default, home, off, t)
    }));
    let busy = s.settings.busy;
    form = form.child(
        row().w_full().child(spacer()).child(
            Button::new(if busy { copy::SAVING } else { copy::SAVE })
                .disabled(off || busy)
                .on_press_tx(|s: &mut Ui, tx: &mut Tx| {
                    s.save_settings(true);
                    tx.wake_after(Duration::ZERO);
                })
                .build(&t.kit)
                .key("settings-save"),
        ),
    );
    c.child(form)
}

/// The settings load, then the server info, then the editor.
fn gate(s: &Ui, t: &Theme) -> El {
    if let Some(e) = &s.settings.error {
        return failure(e, s.settings.retrying, Ui::retry_settings, t);
    }
    let Some(loaded) = &s.settings.loaded else {
        return pending(t);
    };
    match (&s.info.value, &s.info.error) {
        (Some(info), _) => editor(s, loaded, info, t),
        (None, Some(e)) => failure(e, s.info.retrying, Ui::retry_server_info, t),
        (None, None) => pending(t),
    }
}

fn capabilities_card(t: &Theme) -> El {
    titled_card(copy::CAPS_TITLE, copy::CAPS_NOTE, t).child(
        row().child(
            Button::new(copy::CAPS_OPEN)
                .variant(ButtonVariant::Outline)
                .on_press_tx(|s: &mut Ui, tx: &mut Tx| {
                    s.go(Route::Capabilities);
                    tx.set_title(format!("{} · Peektrace", Route::Capabilities.title()));
                    tx.wake_after(Duration::ZERO);
                })
                .build(&t.kit)
                .key("settings-capabilities"),
        ),
    )
}

pub fn page(s: &Ui, t: &Theme) -> El {
    let column = col()
        .key("settings")
        .w_full()
        .max_w(COLUMN_W)
        .gap(16.0)
        .padding(16.0)
        .child(
            col()
                .w_full()
                .gap(2.0)
                .child(text(copy::HEADING).font(ty::LG_SEMI).color(t.fg))
                .child(
                    text(copy::HEADING_NOTE)
                        .font(ty::SM)
                        .color(t.muted_fg)
                        .wrap()
                        .w_full(),
                ),
        )
        .child(gate(s, t))
        .child(capabilities_card(t));
    col()
        .key("settings-scroll")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(col().w_full().items_center().padding(24.0).child(column))
}
