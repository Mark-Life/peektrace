//! Per-day, per-project call counts a cause rate divides by, and the fix
//! text the cause table composes.

use indexmap::IndexMap;

use super::causes::{PATH_FILES, spec};
use super::rules::{RuleCover, Split};
use crate::model::Agent;
use crate::stats::rank::{cmp_str, desc};
use crate::stats::schema::{CallFact, CauseId, Kind};

/// Where a session ran.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Origin {
    pub cwd: Option<String>,
    pub source: Option<String>,
}

/// Counts of one UTC day, agent, project, working dir and source.
#[derive(Debug, Clone)]
pub struct Cell {
    pub agent: Agent,
    pub day: String,
    pub project: String,
    pub cwd: Option<String>,
    pub source: Option<String>,
    pub main: f64,
    pub nested: f64,
}

pub type CellSeries = IndexMap<String, Cell>;

/// A sidechain inside a main transcript is a subagent too.
pub fn is_nested(f: &CallFact) -> bool {
    f.base.kind != Kind::Main || f.side
}

pub fn add_to(split: &mut Split, nested: bool) {
    if nested {
        split.nested += 1.0;
    } else {
        split.main += 1.0;
    }
}

fn day_of(f: &CallFact) -> String {
    f.ts.as_deref()
        .map(|t| crate::js::units::slice(t, 0, 10))
        .unwrap_or_default()
}

pub fn bump_cell(series: &mut CellSeries, f: &CallFact, origin: &Origin) {
    let day = day_of(f);
    let key = [
        day.as_str(),
        f.base.agent.as_str(),
        f.base.project.as_str(),
        origin.cwd.as_deref().unwrap_or(""),
        origin.source.as_deref().unwrap_or(""),
    ]
    .join("\0");
    let nested = is_nested(f);
    let cell = series.entry(key).or_insert_with(|| Cell {
        agent: f.base.agent,
        day,
        project: f.base.project.clone(),
        cwd: origin.cwd.clone(),
        source: origin.source.clone(),
        main: 0.0,
        nested: 0.0,
    });
    if nested {
        cell.nested += 1.0;
    } else {
        cell.main += 1.0;
    }
}

pub fn sum_cells<'a>(
    series: impl IntoIterator<Item = &'a CellSeries>,
    keep: &dyn Fn(&Cell) -> bool,
) -> Split {
    let mut out = Split::default();
    for s in series {
        for cell in s.values().filter(|c| keep(c)) {
            out.main += cell.main;
            out.nested += cell.nested;
        }
    }
    out
}

/// Entries by count desc, key asc, first `cap`.
pub fn top_counts(counts: &IndexMap<String, f64>, cap: usize) -> Vec<(String, f64)> {
    let mut v: Vec<(String, f64)> = counts.iter().map(|(k, n)| (k.clone(), *n)).collect();
    v.sort_by(|a, b| desc(a.1, b.1).then_with(|| cmp_str(&a.0, &b.0)));
    v.truncate(cap);
    v
}

const PATH_BINS: [&str; 14] = [
    "node", "npm", "npx", "pnpm", "yarn", "fnm", "bun", "bunx", "deno", "tsx", "corepack", "cargo",
    "rustc", "go",
];
const FIX_BINS: usize = 4;
const LOCAL_BIN: &str = "~/.local/bin";

fn package_of(b: &str) -> &str {
    match b {
        "strings" => "binutils (strings)",
        "objdump" => "binutils (objdump)",
        "readelf" => "binutils (readelf)",
        "rg" => "ripgrep (rg)",
        "fd" => {
            "fd (on Debian/Ubuntu: apt install fd-find, then mkdir -p ~/.local/bin && ln -s $(command -v fdfind) ~/.local/bin/fd)"
        }
        other => other,
    }
}

fn sourced_fix(b: &str) -> Option<String> {
    (b == "nvm").then(|| format!("nvm is a shell function, not a binary: source $NVM_DIR/nvm.sh in {PATH_FILES}, or put a node version's bin dir on PATH instead"))
}

/// "install jq, binutils (strings); or put node on PATH in ..." for missing bins.
pub fn command_not_found_fix(bins: &[String]) -> Option<String> {
    let top = &bins[..bins.len().min(FIX_BINS)];
    let sourced: Vec<String> = top.iter().filter_map(|b| sourced_fix(b)).collect();
    let install: Vec<&String> = top
        .iter()
        .filter(|b| !(PATH_BINS.contains(&b.as_str()) || sourced_fix(b).is_some()))
        .collect();
    let linked = install.iter().any(|b| *b == "fd");
    let mut on_path: Vec<&str> = top
        .iter()
        .filter(|b| PATH_BINS.contains(&b.as_str()))
        .map(String::as_str)
        .collect();
    if linked {
        on_path.push(LOCAL_BIN);
    }
    let mut parts = Vec::new();
    if !install.is_empty() {
        parts.push(format!(
            "install {}",
            install
                .iter()
                .map(|b| package_of(b))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !on_path.is_empty() {
        parts.push(format!(
            "put {} on PATH in {PATH_FILES}",
            on_path.join(" and ")
        ));
    }
    let mut all = Vec::new();
    if !parts.is_empty() {
        all.push(parts.join(if linked { "; then " } else { "; or " }));
    }
    all.extend(sourced);
    (!all.is_empty()).then(|| all.join("; "))
}

/// The fix for one call's cause, sharpened by its culprit.
pub fn call_fix_of(cause: CauseId, arg: Option<&str>) -> Option<String> {
    let fix = spec(cause).fix.map(str::to_owned);
    let Some(arg) = arg else { return fix };
    match cause {
        CauseId::CommandNotFound => command_not_found_fix(&[arg.to_owned()]).or(fix),
        CauseId::StructuredOutputSchema => {
            Some(format!("{} Missing: '{arg}'.", fix.unwrap_or_default()))
        }
        _ => fix,
    }
}

fn is_under(cwd: &str, dir: &str) -> bool {
    let path = cwd.replace('\\', "/");
    let slashed = dir.replace('\\', "/");
    let base = slashed.strip_suffix('/').unwrap_or(&slashed);
    path == base || path.starts_with(&format!("{base}/"))
}

fn from_source(cover: &RuleCover, cell: &Cell) -> bool {
    match (&cover.source, &cell.source) {
        (None, _) => true,
        (Some((_, is_default)), None) => *is_default,
        (Some((id, _)), Some(s)) => s == id,
    }
}

fn walk_reaches(cover: &RuleCover, dir: &str, cwd: &str) -> bool {
    match &cover.codex {
        None => is_under(cwd, dir),
        Some(c) if c.exact => is_under(cwd, dir) && is_under(dir, cwd),
        Some(c) => is_under(cwd, dir) && !c.stops.iter().any(|s| is_under(cwd, s)),
    }
}

/// True when a memory file with this cover is loaded for the cell's calls.
pub fn reaches(cover: &RuleCover, cell: &Cell, cwd: Option<&str>) -> bool {
    if cell.agent != cover.agent || !from_source(cover, cell) {
        return false;
    }
    match &cover.dir {
        None => true,
        Some(dir) => cwd.is_some_and(|c| walk_reaches(cover, dir, c)),
    }
}
