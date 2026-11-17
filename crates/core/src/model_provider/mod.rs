//! Which company made a model, for the provider logo beside a session.
//!
//! Matches a model id against a committed models.dev snapshot
//! (`providers.tsv`), with no network. `peektrace pricing update
//! --providers <file>` regenerates the snapshot.

use std::collections::HashMap;
use std::sync::LazyLock;

mod build;
#[cfg(test)]
mod tests;

pub use build::{MAKERS, providers_snapshot};

/// A model's maker: `id` names its logo, `name` is for people.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProviderMark {
    pub id: &'static str,
    pub name: &'static str,
}

const SNAPSHOT: &str = include_str!("providers.tsv");

/// Org prefixes (`org/model`, `org.model`) and the maker they name.
const ORGS: [(&str, &str); 24] = [
    ("anthropic", "anthropic"),
    ("openai", "openai"),
    ("google", "google"),
    ("gemini", "google"),
    ("xai", "xai"),
    ("x-ai", "xai"),
    ("zai", "zai"),
    ("z-ai", "zai"),
    ("zai-org", "zai"),
    ("thudm", "zai"),
    ("moonshotai", "moonshotai"),
    ("moonshot", "moonshotai"),
    ("alibaba", "alibaba"),
    ("qwen", "alibaba"),
    ("deepseek", "deepseek"),
    ("deepseek-ai", "deepseek"),
    ("mistral", "mistral"),
    ("mistralai", "mistral"),
    ("minimax", "minimax"),
    ("minimaxai", "minimax"),
    ("meta", "meta"),
    ("meta-llama", "meta"),
    ("nvidia", "nvidia"),
    ("cohere", "cohere"),
];

/// Model family prefixes and their maker, longest first where one prefix
/// starts another.
const FAMILIES: [(&str, &str); 26] = [
    ("claude", "anthropic"),
    ("chatgpt", "openai"),
    ("gpt", "openai"),
    ("codex", "openai"),
    ("o1", "openai"),
    ("o3", "openai"),
    ("o4", "openai"),
    ("gemini", "google"),
    ("gemma", "google"),
    ("grok", "xai"),
    ("glm", "zai"),
    ("zai", "zai"),
    ("kimi", "moonshotai"),
    ("qwen", "alibaba"),
    ("qwq", "alibaba"),
    ("deepseek", "deepseek"),
    ("mistral", "mistral"),
    ("codestral", "mistral"),
    ("devstral", "mistral"),
    ("ministral", "mistral"),
    ("magistral", "mistral"),
    ("pixtral", "mistral"),
    ("minimax", "minimax"),
    ("llama", "meta"),
    ("nemotron", "nvidia"),
    ("command", "cohere"),
];

/// Cloud region prefixes of ids like `us.anthropic.claude-...`.
const REGIONS: [&str; 5] = ["us", "eu", "apac", "global", "jp"];

/// The maker a model id's family names. `o1`-style prefixes need a word
/// break after them, so `o1x` is no match.
fn family(id: &str) -> Option<&'static str> {
    FAMILIES.iter().find_map(|(prefix, maker)| {
        let rest = id.strip_prefix(prefix)?;
        let short = prefix.len() <= 2;
        (!short || rest.is_empty() || rest.starts_with(['-', '.', '_'])).then_some(*maker)
    })
}

struct Snapshot {
    makers: Vec<ProviderMark>,
    /// Lowercase model id to its index in `makers`.
    exact: HashMap<&'static str, usize>,
}

fn parse(text: &'static str) -> Snapshot {
    let mut makers = Vec::new();
    let mut exact = HashMap::new();
    for line in text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let mut cols = line.split('\t');
        let (Some(id), Some(name)) = (cols.next(), cols.next()) else {
            continue;
        };
        let i = makers.len();
        makers.push(ProviderMark { id, name });
        for m in cols
            .next()
            .unwrap_or("")
            .split(' ')
            .filter(|m| !m.is_empty())
        {
            exact.entry(m).or_insert(i);
        }
    }
    Snapshot { makers, exact }
}

static TABLE: LazyLock<Snapshot> = LazyLock::new(|| parse(SNAPSHOT));

fn mark(id: &str) -> Option<ProviderMark> {
    TABLE.makers.iter().find(|m| m.id == id).copied()
}

fn org(prefix: &str) -> Option<ProviderMark> {
    ORGS.iter()
        .find(|(o, _)| *o == prefix)
        .and_then(|(_, maker)| mark(maker))
}

/// The maker of `model`, e.g. Anthropic for `claude-opus-5-5` or NVIDIA
/// for `nvidia/GLM-5.3-Flash-NVFP4`. `None` when unknown.
///
/// Tried in order: an `org/` (or `region.org.`) prefix, the exact id in a
/// maker's own list, then the family prefix (`claude`, `gpt`, `glm`, ...).
/// After an unknown org the part past the last `/` is matched.
pub fn model_provider(model: &str) -> Option<ProviderMark> {
    let lower = model.trim().to_ascii_lowercase();
    let mut id = lower.as_str();
    if let Some((prefix, _)) = id.split_once('/') {
        if let Some(m) = org(prefix) {
            return Some(m);
        }
        id = id.rsplit('/').next().unwrap_or(id);
    }
    let mut dotted = id.split('.');
    let first = dotted.next().unwrap_or("");
    let vendor = if REGIONS.contains(&first) {
        dotted.next()
    } else {
        Some(first)
    };
    if id.contains('.')
        && let Some(m) = vendor.and_then(org)
    {
        return Some(m);
    }
    if id.is_empty() {
        return None;
    }
    TABLE
        .exact
        .get(id)
        .and_then(|&i| TABLE.makers.get(i).copied())
        .or_else(|| family(id).and_then(mark))
}

/// One tooltip line: `"Anthropic · claude-opus-5-5"`, or
/// `"Unknown provider · <model>"`.
pub fn provider_line(model: &str) -> String {
    let name = model_provider(model).map_or("Unknown provider", |m| m.name);
    format!("{name} · {model}")
}
