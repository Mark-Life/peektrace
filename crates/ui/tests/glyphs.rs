//! Every non-ASCII character the UI writes itself must be in the fonts it
//! ships: the web has no system fonts, and native ones differ per OS.

use std::path::{Path, PathBuf};

use skrifa::{FontRef, MetadataProvider};

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts")
}

fn covers(files: &[&str], c: char) -> bool {
    files.iter().any(|f| {
        let bytes = std::fs::read(assets().join(f)).unwrap_or_default();
        FontRef::new(&bytes).is_ok_and(|font| font.charmap().map(c).is_some())
    })
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs")
            && !p
                .file_stem()
                .is_some_and(|s| s.to_string_lossy().contains("test"))
        {
            out.push(p);
        }
    }
}

/// Non-ASCII characters outside comments and before any `#[cfg(test)]` module.
fn ui_chars() -> Vec<(char, String)> {
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut out: Vec<(char, String)> = files
        .iter()
        .flat_map(|p| {
            let src = std::fs::read_to_string(p).unwrap_or_default();
            let name = p.display().to_string();
            src.lines()
                .take_while(|l| !l.contains("#[cfg(test)]"))
                .filter(|l| !l.trim_start().starts_with("//"))
                .flat_map(|l| l.chars().filter(|c| !c.is_ascii()).collect::<Vec<_>>())
                .map(|c| (c, name.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    out.sort_unstable_by_key(|(c, _)| *c);
    out.dedup_by_key(|(c, _)| *c);
    out
}

#[test]
fn shipped_fonts_cover_the_ui_glyphs() {
    let chars = ui_chars();
    assert!(chars.iter().any(|(c, _)| *c == '▾'), "scan found no ▾");
    let missing: Vec<_> = chars
        .iter()
        .filter(|(c, _)| {
            !covers(&["geist.ttf", "symbols.ttf"], *c)
                || !covers(&["geist-mono.ttf", "symbols.ttf"], *c)
        })
        .collect();
    assert!(
        missing.is_empty(),
        "no glyph in the shipped fonts: {missing:?}"
    );
}
