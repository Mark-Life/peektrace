//! Small shared pieces: badges, dots, cards, chips, skeletons, the tile
//! grid and the notice panel.

use rune::{Color, El, Shape, canvas, col, el, row, text};

use crate::theme::{Theme, ty};

/// A filled circle, drawn as a 16-gon.
pub fn dot(size: f32, color: Color) -> El {
    let r = size / 2.0;
    let pts: Vec<[f32; 2]> = (0..16)
        .map(|i| {
            #[expect(clippy::cast_precision_loss, reason = "small loop index")]
            let a = i as f32 / 16.0 * std::f32::consts::TAU;
            [r + r * a.cos(), r + r * a.sin()]
        })
        .collect();
    canvas(Shape::new().fill(pts, color)).size(size, size)
}

/// A small outlined pill in `color`.
pub fn badge(label: &str, color: Color, t: &Theme) -> El {
    row()
        .h(18.0)
        .px(6.0)
        .radius(9.0)
        .center_y()
        .border(1.0, color.alpha(0.5))
        .fill(color.alpha(if t.dark { 0.12 } else { 0.08 }))
        .child(text(label.to_owned()).font(ty::TINY).color(color))
}

/// A bordered card.
pub fn card(t: &Theme) -> El {
    col()
        .w_full()
        .radius(12.0)
        .border(1.0, t.border)
        .fill(t.bg)
        .padding(16.0)
        .gap(12.0)
}

/// A chip: outlined, text only; pressable when given a handler afterwards.
pub fn chip(label: impl Into<String>, color: Color, t: &Theme) -> El {
    row()
        .h(24.0)
        .px(8.0)
        .radius(12.0)
        .center_y()
        .border(1.0, t.border)
        .child(text(label.into()).font(ty::TINY).color(color))
}

/// A centred title and line of text, for empty and loading panes.
pub fn notice(title: &str, body: &str, t: &Theme) -> El {
    col()
        .w_full()
        .h_full()
        .center()
        .gap(6.0)
        .child(text(title.to_owned()).font(ty::BASE_SEMI).color(t.fg))
        .child(
            text(body.to_owned())
                .font(ty::SM)
                .color(t.muted_fg)
                .wrap()
                .align(rune::Align::Center)
                .max_w(520.0),
        )
}

/// A muted filled block standing in for content that is loading.
pub fn skeleton(w: Option<f32>, h: f32, t: &Theme) -> El {
    let b = el().h(h).radius(6.0).fill(t.muted);
    match w {
        Some(w) => b.w(w).max_w(f32::MAX),
        None => b.w_full(),
    }
}

/// `items` in `cols` equal columns; a short last row keeps the same widths.
pub fn grid(cols: usize, gap: f32, items: impl IntoIterator<Item = El>) -> El {
    col().w_full().equal_columns(cols).gap(gap).children(items)
}

/// The muted line a loading pane reads out.
pub fn status(s: &str, t: &Theme) -> El {
    text(s.to_owned())
        .font(ty::SM)
        .color(t.muted_fg)
        .wrap()
        .role("status")
        .min_w(0.0)
}
