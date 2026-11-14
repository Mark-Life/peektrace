//! Copy and prompt buttons: copy a line, then read "Copied" for a moment.

use rune::{El, Tx};
use rune_kit::glyphs;

use super::small_button;
use crate::state::Ui;
use crate::theme::Theme;

/// A copy button keyed `key`; `id` names it among every copy button, so
/// only the pressed one says "Copied". `describe` is the tooltip, else the
/// text itself.
pub fn copy_button(
    s: &Ui,
    key: &str,
    id: String,
    label: &str,
    text: &str,
    describe: Option<String>,
    t: &Theme,
) -> El {
    let copied = s.stats.copied(&id);
    let help = describe.unwrap_or_else(|| text.to_owned());
    let text = text.to_owned();
    small_button(if copied { "Copied" } else { label }, false, t)
        .icon(if copied {
            &glyphs::CHECK
        } else {
            &glyphs::COPY
        })
        .help(help)
        .on_press_tx(move |s: &mut Ui, tx: &mut Tx| s.copy_text(id.clone(), text.clone(), tx))
        .build(&t.kit)
        .key(key.to_owned())
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
