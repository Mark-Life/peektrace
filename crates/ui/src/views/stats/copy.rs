//! Copy and prompt buttons: copy a line, then read "Copied" for a moment,
//! or "Copy failed" over the text to copy by hand.

use rune::{El, ScrollSpec, Tx, col, text};
use rune_kit::glyphs;

use super::small_button;
use crate::state::Ui;
use crate::state_stats::CopyState;
use crate::theme::{Theme, ty};

/// A copy button keyed `key`; `id` names it among every copy button, so
/// only the pressed one changes. `describe` is the tooltip, else the text
/// itself.
pub fn copy_button(
    s: &Ui,
    key: &str,
    id: String,
    label: &str,
    text: &str,
    describe: Option<String>,
    t: &Theme,
) -> El {
    let shown = s.stats.copy_shown(&id);
    let (caption, icon) = match shown {
        Some(CopyState::Copied { .. }) => ("Copied", &glyphs::CHECK),
        Some(CopyState::Failed) => ("Copy failed", &glyphs::CLOSE),
        Some(CopyState::Copying) | None => (label, &glyphs::COPY),
    };
    let help = describe.unwrap_or_else(|| text.to_owned());
    let copy = text.to_owned();
    let button = small_button(caption, false, t)
        .icon(icon)
        .help(help)
        .on_press_tx(move |s: &mut Ui, tx: &mut Tx| s.copy_text(id.clone(), copy.clone(), tx))
        .build(&t.kit)
        .key(key.to_owned());
    if shown != Some(CopyState::Failed) {
        return button;
    }
    col()
        .gap(4.0)
        .child(button)
        .child(manual(key, label, text, t))
}

/// The text of a failed copy, to select and copy by hand.
fn manual(key: &str, label: &str, body: &str, t: &Theme) -> El {
    col()
        .key(format!("{key}-manual"))
        .w(256.0)
        .max_w(f32::MAX)
        .max_h(64.0)
        .padding(4.0)
        .radius(4.0)
        .border(1.0, t.border)
        .fill(t.muted)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            text(body.to_owned())
                .font(ty::MONO_XS)
                .color(t.fg)
                .wrap()
                .selectable(true)
                .label(format!("{label}: select and copy")),
        )
}

/// "Copy prompt" for `subject`; no button when the row has no prompt.
pub fn prompt_button(
    s: &Ui,
    key: &str,
    id: String,
    subject: &str,
    text: Option<&str>,
    t: &Theme,
) -> Option<El> {
    text.map(|text| {
        copy_button(
            s,
            key,
            id,
            "Copy prompt",
            text,
            Some(format!("Copy prompt: investigate {subject}")),
            t,
        )
    })
}
