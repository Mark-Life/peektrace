//! Line diff of the before and after text an edit tool records: one block
//! with unchanged lines for context and long unchanged stretches folded.
//! Line numbers count from 1 within the snippet.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowKind {
    Add,
    Ctx,
    Del,
    Gap,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiffRow {
    pub kind: RowKind,
    pub old_no: Option<usize>,
    pub new_no: Option<usize>,
    pub text: String,
    /// Unchanged lines a gap stands for.
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LineDiff {
    pub added: usize,
    pub removed: usize,
    pub rows: Vec<DiffRow>,
}

const CONTEXT: usize = 3;
/// Past this many LCS cells the diff is a whole-block replace.
const MAX_CELLS: usize = 1_000_000;

fn split_lines(s: &str) -> Vec<&str> {
    if s.is_empty() {
        Vec::new()
    } else {
        s.split('\n').collect()
    }
}

fn row(kind: RowKind, old_no: Option<usize>, new_no: Option<usize>, text: &str) -> DiffRow {
    DiffRow {
        kind,
        old_no,
        new_no,
        text: text.to_owned(),
        count: 0,
    }
}

fn replace_all(before: &[&str], after: &[&str]) -> Vec<DiffRow> {
    before
        .iter()
        .enumerate()
        .map(|(i, t)| row(RowKind::Del, Some(i + 1), None, t))
        .chain(
            after
                .iter()
                .enumerate()
                .map(|(i, t)| row(RowKind::Add, None, Some(i + 1), t)),
        )
        .collect()
}

fn align_rows(before: &[&str], after: &[&str]) -> Vec<DiffRow> {
    let width = after.len() + 1;
    let mut table = vec![0_u32; (before.len() + 1) * width];
    for i in (0..before.len()).rev() {
        for j in (0..after.len()).rev() {
            let here = i * width + j;
            table[here] = if before[i] == after[j] {
                table[here + width + 1] + 1
            } else {
                table[here + width].max(table[here + 1])
            };
        }
    }
    let mut rows = Vec::with_capacity(before.len() + after.len());
    let (mut i, mut j) = (0, 0);
    while i < before.len() && j < after.len() {
        if before[i] == after[j] {
            rows.push(row(RowKind::Ctx, Some(i + 1), Some(j + 1), before[i]));
            i += 1;
            j += 1;
        } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
            rows.push(row(RowKind::Del, Some(i + 1), None, before[i]));
            i += 1;
        } else {
            rows.push(row(RowKind::Add, None, Some(j + 1), after[j]));
            j += 1;
        }
    }
    rows.extend((i..before.len()).map(|i| row(RowKind::Del, Some(i + 1), None, before[i])));
    rows.extend((j..after.len()).map(|j| row(RowKind::Add, None, Some(j + 1), after[j])));
    rows
}

fn with_gaps(rows: Vec<DiffRow>) -> Vec<DiffRow> {
    let mut keep = vec![false; rows.len()];
    for (i, r) in rows.iter().enumerate() {
        if r.kind != RowKind::Ctx {
            let lo = i.saturating_sub(CONTEXT);
            let hi = (i + CONTEXT).min(rows.len() - 1);
            keep[lo..=hi].iter_mut().for_each(|k| *k = true);
        }
    }
    let mut out = Vec::new();
    let mut skipped = 0;
    let gap = |count| DiffRow {
        kind: RowKind::Gap,
        old_no: None,
        new_no: None,
        text: String::new(),
        count,
    };
    for (r, k) in rows.into_iter().zip(keep) {
        if k {
            if skipped > 0 {
                out.push(gap(skipped));
                skipped = 0;
            }
            out.push(r);
        } else {
            skipped += 1;
        }
    }
    if skipped > 0 {
        out.push(gap(skipped));
    }
    out
}

pub fn diff_lines(before: &str, after: &str) -> LineDiff {
    let (a, b) = (split_lines(before), split_lines(after));
    let rows = if (a.len() + 1).saturating_mul(b.len() + 1) > MAX_CELLS {
        replace_all(&a, &b)
    } else {
        align_rows(&a, &b)
    };
    LineDiff {
        added: rows.iter().filter(|r| r.kind == RowKind::Add).count(),
        removed: rows.iter().filter(|r| r.kind == RowKind::Del).count(),
        rows: with_gaps(rows),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_folds() {
        use RowKind::{Add, Ctx, Del, Gap};
        let before = (1..=10)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let after = before.replace("5\n", "five\n");
        let d = diff_lines(&before, &after);
        assert_eq!((d.added, d.removed), (1, 1));
        let kinds: Vec<RowKind> = d.rows.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, [Gap, Ctx, Ctx, Ctx, Del, Add, Ctx, Ctx, Ctx, Gap]);
        assert_eq!(d.rows[0].count, 1);
        assert_eq!(diff_lines("", "a\nb").added, 2);
    }
}
