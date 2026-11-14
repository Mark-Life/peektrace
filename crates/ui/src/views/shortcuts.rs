//! App-wide overlays and the rail footer: the shortcuts dialog, the save
//! toast, the theme toggle and the shortcuts button.

use rune::{El, col, icon, row, text};
use rune_kit::{Dialog, Keybind, Toast, glyphs};

use crate::model::shortcuts::{
    GROUPS, HELP_BUTTON, HELP_NOTE, HELP_TITLE, SHORTCUTS, group_note, key_label, shortcut,
};
use crate::shell::MAC;
use crate::state::Ui;
use crate::theme::{Theme, ty};

const HELP_W: f32 = 448.0;

/// A shortcut's keycaps, from the table.
pub fn keycaps(id: &str) -> Vec<String> {
    shortcut(id).map_or_else(Vec::new, |s| {
        s.keys.iter().map(|k| key_label(k, MAC)).collect()
    })
}

fn list(t: &Theme) -> El {
    col()
        .key("shortcuts-list")
        .w_full()
        .gap(16.0)
        .children(GROUPS.into_iter().map(|g| {
            let mut section = col()
                .w_full()
                .gap(4.0)
                .role("group")
                .label(g.as_str())
                .child(
                    text(g.as_str().to_uppercase())
                        .font(ty::XS_MEDIUM)
                        .color(t.muted_fg),
                );
            if let Some(note) = group_note(g) {
                section = section.child(text(note).font(ty::XS).color(t.muted_fg).wrap().w_full());
            }
            section.children(SHORTCUTS.iter().filter(|s| s.group == g).map(|s| {
                row()
                    .key(format!("shortcut-{}", s.id))
                    .w_full()
                    .py(6.0)
                    .gap(16.0)
                    .center_y()
                    .border_sides([0.0, 0.0, 1.0, 0.0], t.border)
                    .child(text(s.label).font(ty::SM).color(t.fg).grow(1.0))
                    .child(Keybind::new(&keycaps(s.id)).build(&t.kit))
            }))
        }))
}

/// The `?` dialog: every shortcut, grouped.
pub fn help(s: &Ui, t: &Theme, layer: &mut Vec<El>) {
    Dialog::new("shortcuts", HELP_TITLE)
        .description(HELP_NOTE)
        .width(HELP_W)
        .open(s.help_open)
        .fit()
        .on_close(|s: &mut Ui| s.help_open = false)
        .child(col().w_full().px(16.0).pb(16.0).child(list(t)))
        .build(&t.kit, layer);
}

/// The save toast, top right.
pub fn toast(s: &Ui, t: &Theme) -> Option<El> {
    let toast = s.toast.as_ref()?;
    let mut b = Toast::new("toast", toast.title.clone()).on_close(|s: &mut Ui| s.toast = None);
    if let Some(d) = &toast.description {
        b = b.description(d.clone());
    }
    if toast.error {
        b = b.lead(icon(&glyphs::CIRCLE_X, 16.0).color(t.destructive));
    }
    Some(
        col()
            .key("toasts")
            .pin(None, Some(16.0), Some(16.0), None)
            .child(b.build(&t.kit)),
    )
}

fn footer_button(body: El, t: &Theme) -> El {
    t.kit
        .interactive(
            row()
                .h(28.0)
                .px(8.0)
                .gap(6.0)
                .radius(6.0)
                .center_y()
                .child(body),
        )
        .role("button")
        .focusable(true)
}

/// The rail footer: theme toggle and shortcuts, icon only on the collapsed rail.
pub fn footer(s: &Ui, t: &Theme) -> El {
    let press = s.theme_press();
    let theme_label = format!("Switch to {} theme", press.appearance.as_str());
    let theme = footer_button(text(press.appearance.label()).font(ty::XS).color(t.fg), t)
        .key("theme-toggle")
        .label(theme_label.clone())
        .help(theme_label)
        .help_keys(keycaps("theme"))
        .on_press(|s: &mut Ui| s.flip_theme());
    let mut keys = row()
        .gap(6.0)
        .center_y()
        .child(icon(&glyphs::KEYBOARD, 16.0).color(t.fg).a11y_hidden());
    if !s.nav_collapsed {
        keys = keys.child(text(HELP_BUTTON).font(ty::XS).color(t.fg));
    }
    let help = footer_button(keys, t)
        .key("shortcuts-button")
        .label(HELP_TITLE)
        .help(HELP_TITLE)
        .help_keys(keycaps("help"))
        .on_press(|s: &mut Ui| s.help_open = true);
    if s.nav_collapsed {
        col().gap(8.0).items_center().child(theme).child(help)
    } else {
        row().gap(8.0).center_y().child(theme).child(help)
    }
}
