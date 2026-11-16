//! Page pieces the inspector's `ResultView`, `Alert` and code-marked copy
//! map to: the pending skeleton, the failure alert with Retry, alerts and
//! text with inline code.

use rune::{Color, El, Span, TextStyle, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize};

use crate::model::settings::{Seg, copy, wire_error_message};
use crate::source::LoadError;
use crate::state::Ui;
use crate::theme::{Theme, ty};

/// The skeleton while a first load runs.
pub fn pending(t: &Theme) -> El {
    let block = |w: Option<f32>, h: f32| {
        let b = el().h(h).radius(6.0).fill(t.muted);
        match w {
            Some(w) => b.w(w),
            None => b.w_full(),
        }
    };
    col()
        .key("result-pending")
        .w_full()
        .gap(12.0)
        .child(block(Some(192.0), 32.0))
        .child(block(None, 128.0))
        .child(block(None, 128.0))
}

/// A failed load: the typed error's sentence, else the transport text, and
/// a Retry that says so while it runs.
pub fn failure(e: &LoadError, retrying: bool, retry: fn(&mut Ui), t: &Theme) -> El {
    let (title, body) = match e {
        LoadError::Api(api) => (copy::TYPED_FAILURE, wire_error_message(api)),
        other => (copy::UNTYPED_FAILURE, other.to_string()),
    };
    let button = Button::new(if retrying {
        copy::RETRYING
    } else {
        copy::RETRY
    })
    .variant(ButtonVariant::Outline)
    .control_size(ControlSize::Small)
    .disabled(retrying)
    .on_press(retry)
    .build(&t.kit)
    .key("result-retry");
    alert(
        title,
        text(body).font(ty::SM).color(t.destructive).wrap().w_full(),
        None,
        true,
        t,
    )
    .key("result-error")
    .child(row().pt(8.0).child(button))
}

/// A bordered notice: optional lead glyph, title, then `body`.
pub fn alert(title: &str, body: El, lead: Option<El>, destructive: bool, t: &Theme) -> El {
    let ink = if destructive { t.destructive } else { t.fg };
    let mut head = row().w_full().gap(8.0).center_y();
    if let Some(l) = lead {
        head = head.child(l);
    }
    head = head.child(text(title.to_owned()).font(ty::SM_MEDIUM).color(ink));
    col()
        .w_full()
        .radius(8.0)
        .border(1.0, t.border)
        .fill(t.card)
        .px(16.0)
        .py(12.0)
        .gap(4.0)
        .child(head)
        .child(body)
}

/// `segs` as one wrapping text, code runs in mono on a muted fill.
pub fn rich(segs: &[Seg], font: TextStyle, color: Color, t: &Theme) -> El {
    let mut out = String::new();
    let mut spans = Vec::new();
    for seg in segs {
        match seg {
            Seg::Plain(s) => out.push_str(s),
            Seg::Code(s) => {
                let start = out.len();
                out.push_str(s);
                spans.push(Span::new(start, out.len()).mono().bg(t.muted, 2.0));
            }
        }
    }
    text(out).font(font).color(color).spans(spans).wrap()
}
