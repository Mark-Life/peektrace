//! The memory list as flat rows: one header per vault, then its entries
//! that pass the search and type filter.

use peektrace_core::memory::types::{MemoryEntry, MemoryVault};

/// Type-filter cycle: `all`, each documented type, then untyped.
pub const TYPE_FILTERS: [&str; 6] = ["all", "user", "feedback", "project", "reference", "untyped"];

pub fn next_type(current: &str) -> &'static str {
    let at = TYPE_FILTERS.iter().position(|t| *t == current).unwrap_or(0);
    TYPE_FILTERS[(at + 1) % TYPE_FILTERS.len()]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Header { vault: usize, matched: usize },
    Entry { vault: usize, entry: usize },
}

/// True when `entry` passes the lowercased query and the type filter.
pub fn entry_matches(entry: &MemoryEntry, query: &str, ty: &str) -> bool {
    if ty != "all" && entry.kind().unwrap_or("untyped") != ty {
        return false;
    }
    if query.is_empty() {
        return true;
    }
    format!(
        "{} {} {} {}",
        entry.slug,
        entry.name().unwrap_or(""),
        entry.description().unwrap_or(""),
        entry.body
    )
    .to_lowercase()
    .contains(query)
}

/// Header and entry rows; with a filter on, vaults with no match drop out.
pub fn build_rows(vaults: &[MemoryVault], query: &str, ty: &str) -> Vec<Row> {
    let filtering = !query.is_empty() || ty != "all";
    let mut rows = Vec::new();
    for (v, vault) in vaults.iter().enumerate() {
        let matched: Vec<usize> = vault
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| entry_matches(e, query, ty))
            .map(|(i, _)| i)
            .collect();
        if filtering && matched.is_empty() {
            continue;
        }
        rows.push(Row::Header {
            vault: v,
            matched: matched.len(),
        });
        rows.extend(
            matched
                .into_iter()
                .map(|entry| Row::Entry { vault: v, entry }),
        );
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_cycle_wraps() {
        assert_eq!(next_type("all"), "user");
        assert_eq!(next_type("untyped"), "all");
    }
}
