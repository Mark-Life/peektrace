//! Drawing primitives: a row of styled segments written into the buffer and
//! clipped to its width, rounded panels with an inline title, and word wrap.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::fmt::{char_width, width};
use crate::theme;

/// One styled run of text.
pub type Seg = (String, Style);

pub fn seg(text: impl Into<String>, style: Style) -> Seg {
    (text.into(), style)
}

/// Display width of a row of segments.
pub fn segs_width(segs: &[Seg]) -> usize {
    segs.iter().map(|(t, _)| width(t)).sum()
}

/// Write `segs` at `(x, y)`, clipped to `w` columns; returns columns used.
pub fn put(buf: &mut Buffer, x: u16, y: u16, w: u16, segs: &[Seg]) -> u16 {
    let area = buf.area;
    if y < area.y || y >= area.bottom() || x >= area.right() {
        return 0;
    }
    let limit = x.saturating_add(w).min(area.right());
    let mut cx = x;
    for (text, style) in segs {
        for ch in text.chars() {
            if ch == '\n' {
                continue;
            }
            let cw = u16::try_from(char_width(ch)).unwrap_or(0);
            if cw == 0 {
                continue;
            }
            if cx + cw > limit {
                return cx - x;
            }
            if let Some(cell) = buf.cell_mut((cx, y)) {
                cell.set_char(ch).set_style(*style);
            }
            if cw == 2
                && let Some(cell) = buf.cell_mut((cx + 1, y))
            {
                cell.set_symbol("").set_style(*style);
            }
            cx += cw;
        }
    }
    cx - x
}

/// Left segments, then right segments flush to the right edge; the left
/// side is clipped first so the right side always shows.
pub fn put_lr(buf: &mut Buffer, x: u16, y: u16, w: u16, left: &[Seg], right: &[Seg]) {
    let rw = u16::try_from(segs_width(right)).unwrap_or(u16::MAX).min(w);
    put(buf, x, y, w - rw, left);
    put(buf, x + w - rw, y, rw, right);
}

/// Paint a style over a whole row (selection background).
pub fn fill_row(buf: &mut Buffer, x: u16, y: u16, w: u16, style: Style) {
    buf.set_style(Rect::new(x, y, w, 1).intersection(buf.area), style);
}

/// A rounded border with `─Title` after the top-left corner; returns the
/// area inside the border.
pub fn frame(
    buf: &mut Buffer,
    area: Rect,
    title: Option<&str>,
    border: Style,
    title_style: Style,
) -> Rect {
    if area.width < 2 || area.height < 2 {
        return Rect::new(area.x, area.y, 0, 0);
    }
    let (l, r, t, b) = (area.x, area.right() - 1, area.y, area.bottom() - 1);
    for x in l + 1..r {
        buf[(x, t)].set_char('─').set_style(border);
        buf[(x, b)].set_char('─').set_style(border);
    }
    for y in t + 1..b {
        buf[(l, y)].set_char('│').set_style(border);
        buf[(r, y)].set_char('│').set_style(border);
    }
    buf[(l, t)].set_char('╭').set_style(border);
    buf[(r, t)].set_char('╮').set_style(border);
    buf[(l, b)].set_char('╰').set_style(border);
    buf[(r, b)].set_char('╯').set_style(border);
    if let Some(title) = title
        && area.width > 3
    {
        put(buf, l + 2, t, area.width - 3, &[seg(title, title_style)]);
    }
    Rect::new(l + 1, t + 1, area.width - 2, area.height - 2)
}

/// A panel: rounded border, brighter when focused, then `pad` cells of
/// padding inside.
pub fn panel(buf: &mut Buffer, area: Rect, title: &str, focused: bool, pad: u16) -> Rect {
    let (border, title_style) = if focused {
        (theme::fg(theme::BORDER_ACTIVE), theme::fg(theme::PRIMARY))
    } else {
        (theme::fg(theme::BORDER), theme::fg(theme::TEXT_DIM))
    };
    let inner = frame(buf, area, Some(title), border, title_style);
    inset(inner, pad, pad)
}

/// `r` shrunk by `x` columns and `y` rows on each side.
pub fn inset(r: Rect, x: u16, y: u16) -> Rect {
    Rect::new(
        r.x + x.min(r.width / 2),
        r.y + y.min(r.height / 2),
        r.width.saturating_sub(2 * x),
        r.height.saturating_sub(2 * y),
    )
}

/// Word-wrap styled segments to `w` columns. Breaks after a space when the
/// line has one, else mid-word; `\n` starts a new row.
pub fn wrap(segs: &[Seg], w: usize) -> Vec<Vec<Seg>> {
    let w = w.max(1);
    let mut rows: Vec<Vec<(char, Style)>> = vec![Vec::new()];
    let mut used = 0usize;
    let mut last_space: Option<usize> = None;
    for (text, style) in segs {
        for ch in text.chars() {
            if ch == '\n' {
                rows.push(Vec::new());
                used = 0;
                last_space = None;
                continue;
            }
            let cw = char_width(ch);
            if used + cw > w && used > 0 {
                let row = rows.last_mut().map(std::mem::take).unwrap_or_default();
                let (head, tail) = match last_space {
                    Some(at) if ch != ' ' && at + 1 < row.len() => {
                        let mut head = row;
                        let tail = head.split_off(at + 1);
                        (head, tail)
                    }
                    _ => (row, Vec::new()),
                };
                if let Some(last) = rows.last_mut() {
                    *last = head;
                }
                used = tail.iter().map(|(c, _)| char_width(*c)).sum();
                rows.push(tail);
                last_space = None;
                if ch == ' ' {
                    continue;
                }
            }
            if ch == ' ' {
                last_space = rows.last().map(Vec::len);
            }
            if let Some(row) = rows.last_mut() {
                row.push((ch, *style));
            }
            used += cw;
        }
    }
    rows.into_iter().map(|r| merge(&r)).collect()
}

fn merge(cells: &[(char, Style)]) -> Vec<Seg> {
    let mut out: Vec<Seg> = Vec::new();
    for (c, s) in cells {
        match out.last_mut() {
            Some((t, st)) if st == s => t.push(*c),
            _ => out.push((c.to_string(), *s)),
        }
    }
    out
}

/// Plain word wrap of one string.
pub fn wrap_text(text: &str, w: usize, style: Style) -> Vec<Vec<Seg>> {
    wrap(&[seg(text, style)], w)
}

/// Break segments into rows of exactly `w` columns (the last may be
/// shorter), never at word boundaries: for URLs that must stay whole.
pub fn hard_wrap(segs: &[Seg], w: usize) -> Vec<Vec<Seg>> {
    let w = w.max(1);
    let mut rows: Vec<Vec<(char, Style)>> = vec![Vec::new()];
    let mut used = 0;
    for (text, style) in segs {
        for ch in text.chars() {
            let cw = char_width(ch);
            if used + cw > w && used > 0 {
                rows.push(Vec::new());
                used = 0;
            }
            if let Some(r) = rows.last_mut() {
                r.push((ch, *style));
            }
            used += cw;
        }
    }
    rows.iter().map(|r| merge(r)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(rows: &[Vec<Seg>]) -> Vec<String> {
        rows.iter()
            .map(|r| r.iter().map(|(t, _)| t.as_str()).collect())
            .collect()
    }

    #[test]
    fn wraps_on_words() {
        let s = Style::new();
        assert_eq!(
            plain(&wrap_text(
                "Browse sessions and open a transcript with full history.",
                38,
                s
            )),
            vec![
                "Browse sessions and open a transcript ",
                "with full history."
            ]
        );
        assert_eq!(
            plain(&wrap_text("abcdefgh", 3, s)),
            vec!["abc", "def", "gh"]
        );
        assert_eq!(plain(&wrap_text("a\n\nb", 3, s)), vec!["a", "", "b"]);
    }
}
