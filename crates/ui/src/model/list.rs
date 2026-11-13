//! The session rail's pure rules: filtering, ordering and row labels.

use std::cmp::Ordering;
use std::collections::HashSet;

use icu_collator::options::CollatorOptions;
use icu_collator::{CollatorBorrowed, CollatorPreferences};
use peektrace_protocol::AgentId;
use peektrace_protocol::view::SessionHeader;

use peektrace_js::time::{MS_PER_DAY as DAY_MS, Utc, day_key};

fn parse_date(s: &str) -> Option<i64> {
    peektrace_js::time::parse_date(s, &Utc)
}

/// A session whose last line is newer than this counts as live.
pub const LIVE_MS: f64 = 300_000.0;
/// Clock skew tolerated for a last line stamped slightly in the future.
pub const SKEW_MS: f64 = 60_000.0;
const MAX_SPAN_DAYS: usize = 366;
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `String.prototype.localeCompare` with no arguments: root collation.
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    thread_local! {
        static ROOT: Option<CollatorBorrowed<'static>> =
            CollatorBorrowed::try_new(CollatorPreferences::default(), CollatorOptions::default()).ok();
    }
    ROOT.with(|c| c.as_ref().map_or_else(|| a.cmp(b), |c| c.compare(a, b)))
}

/// A picked calendar range as `YYYY-MM-DD` days; `to` unset is one day.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DayRange {
    pub from: String,
    pub to: Option<String>,
}

/// The facets that narrow the rail, plus the title/id query. `None` is any.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Filters {
    pub query: String,
    pub agent: Option<AgentId>,
    pub project: Option<String>,
    pub branch: Option<String>,
    pub model: Option<String>,
    pub source: Option<String>,
    pub range: Option<DayRange>,
}

/// One filter, for clearing the ones that hide a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterName {
    Query,
    Agent,
    Project,
    Branch,
    Model,
    Source,
    Range,
}

impl FilterName {
    pub const ALL: [Self; 7] = [
        Self::Query,
        Self::Agent,
        Self::Project,
        Self::Branch,
        Self::Model,
        Self::Source,
        Self::Range,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Agent => "agent",
            Self::Project => "project",
            Self::Branch => "branch",
            Self::Model => "model",
            Self::Source => "source",
            Self::Range => "range",
        }
    }
}

impl Filters {
    /// Only filter `name` of `self`, the rest at their defaults.
    #[must_use]
    pub fn only(&self, name: FilterName) -> Self {
        let mut f = Self::default();
        match name {
            FilterName::Query => f.query.clone_from(&self.query),
            FilterName::Agent => f.agent = self.agent,
            FilterName::Project => f.project.clone_from(&self.project),
            FilterName::Branch => f.branch.clone_from(&self.branch),
            FilterName::Model => f.model.clone_from(&self.model),
            FilterName::Source => f.source.clone_from(&self.source),
            FilterName::Range => f.range.clone_from(&self.range),
        }
        f
    }

    /// `self` with filter `name` back at its default.
    #[must_use]
    pub fn without(mut self, name: FilterName) -> Self {
        match name {
            FilterName::Query => self.query.clear(),
            FilterName::Agent => self.agent = None,
            FilterName::Project => self.project = None,
            FilterName::Branch => self.branch = None,
            FilterName::Model => self.model = None,
            FilterName::Source => self.source = None,
            FilterName::Range => self.range = None,
        }
        self
    }
}

/// Display label of an agent.
pub const fn agent_label(a: AgentId) -> &'static str {
    match a {
        AgentId::Claude => "Claude",
        AgentId::Codex => "Codex",
        AgentId::Pi => "Pi",
        AgentId::Opencode => "OpenCode",
    }
}

/// How many facets (the query counts as one) constrain the list.
pub fn active_count(f: &Filters) -> usize {
    [
        !f.query.is_empty(),
        f.range.is_some(),
        f.agent.is_some(),
        f.project.is_some(),
        f.branch.is_some(),
        f.model.is_some(),
        f.source.is_some(),
    ]
    .into_iter()
    .filter(|b| *b)
    .count()
}

fn is_day_key(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && [0, 1, 2, 3, 5, 6, 8, 9]
            .iter()
            .all(|&i| b[i].is_ascii_digit())
}

/// The first 10 chars of `iso` when they are a real calendar day.
fn day_of(iso: Option<&str>) -> Option<&str> {
    let day = iso?.get(..10)?;
    (is_day_key(day) && parse_date(day).is_some()).then_some(day)
}

/// `Jul 9, 08:18` read off the ISO string, or `—`.
pub fn fmt_stamp(iso: Option<&str>) -> String {
    let parts = iso.and_then(|s| {
        let b = s.as_bytes();
        let digits =
            |r: std::ops::Range<usize>| b.get(r).is_some_and(|d| d.iter().all(u8::is_ascii_digit));
        let ok = digits(0..4)
            && b.get(4) == Some(&b'-')
            && digits(5..7)
            && b.get(7) == Some(&b'-')
            && digits(8..10)
            && b.get(10) == Some(&b'T')
            && digits(11..13)
            && b.get(13) == Some(&b':')
            && digits(14..16);
        ok.then(|| (&s[5..7], &s[8..10], &s[11..16]))
    });
    let Some((mm, dd, hm)) = parts else {
        return "—".to_owned();
    };
    let month = mm
        .parse::<usize>()
        .ok()
        .and_then(|m| m.checked_sub(1))
        .and_then(|m| MONTHS.get(m));
    match month {
        Some(m) => format!("{m} {}, {hm}", dd.parse::<u32>().unwrap_or(0)),
        None => "—".to_owned(),
    }
}

/// `updatedAt ?? startedAt`; an empty `updatedAt` stands for a missing one.
fn last_stamp(h: &SessionHeader) -> Option<&str> {
    Some(h.updated_at.as_str())
        .filter(|s| !s.is_empty())
        .or(h.started_at.as_deref())
        .filter(|s| !s.is_empty())
}

/// The row stamp's tooltip: last activity, plus the start when it differs.
pub fn stamp_title(h: &SessionHeader) -> String {
    let Some(last) = last_stamp(h) else {
        return "unknown time".to_owned();
    };
    let started = match h.started_at.as_deref() {
        Some(s) if !s.is_empty() && s != last => format!(" · started {}", fmt_stamp(Some(s))),
        _ => String::new(),
    };
    format!("Last activity {}{started}", fmt_stamp(Some(last)))
}

/// The row headline: the title, else `<Agent> session`.
pub fn title_of(h: &SessionHeader) -> String {
    match h.title.as_deref() {
        Some(t) if !t.is_empty() => t.to_owned(),
        _ => format!("{} session", agent_label(h.agent)),
    }
}

fn segments(path: &str) -> impl DoubleEndedIterator<Item = &str> {
    path.split(['/', '\\']).filter(|s| !s.is_empty())
}

fn cwd(h: &SessionHeader) -> Option<&str> {
    h.cwd.as_deref().filter(|c| !c.is_empty())
}

/// The folder the session ran in, else the stored project slug.
pub fn project_label(h: &SessionHeader) -> &str {
    cwd(h)
        .and_then(|c| segments(c).next_back())
        .unwrap_or(&h.project)
}

/// The project facet key: the session's folder, else the slug.
pub fn project_key(h: &SessionHeader) -> &str {
    cwd(h).unwrap_or(&h.project)
}

/// A filter option: `value` matches the header, `label` is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterOption {
    pub value: String,
    pub label: String,
}

fn collisions<'a>(labels: impl IntoIterator<Item = &'a str>) -> HashSet<&'a str> {
    let mut seen = HashSet::new();
    let mut dup = HashSet::new();
    for l in labels {
        if !seen.insert(l) {
            dup.insert(l);
        }
    }
    dup
}

/// Project options, one per folder. Shared folder names get their parent
/// too; still-equal labels fall back to the full key.
pub fn project_options(headers: &[SessionHeader]) -> Vec<FilterOption> {
    let mut seen = HashSet::new();
    let entries: Vec<(&str, &SessionHeader)> = headers
        .iter()
        .map(|h| (project_key(h), h))
        .filter(|(k, _)| !k.is_empty() && seen.insert(*k))
        .collect();
    let short = collisions(entries.iter().map(|(_, h)| project_label(h)));
    let first: Vec<FilterOption> = entries
        .iter()
        .map(|(value, h)| {
            let label = project_label(h);
            let label = match cwd(h) {
                Some(c) if short.contains(label) => {
                    let segs: Vec<&str> = segments(c).collect();
                    segs[segs.len().saturating_sub(2)..].join("/")
                }
                _ => label.to_owned(),
            };
            FilterOption {
                value: (*value).to_owned(),
                label,
            }
        })
        .collect();
    let long: HashSet<String> = collisions(first.iter().map(|o| o.label.as_str()))
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut out: Vec<FilterOption> = first
        .into_iter()
        .map(|o| {
            if long.contains(&o.label) {
                FilterOption {
                    label: o.value.clone(),
                    ..o
                }
            } else {
                o
            }
        })
        .collect();
    out.sort_by(|a, b| locale_compare(&a.label, &b.label));
    out
}

/// The branch worth showing: none for empty or a detached `HEAD`.
pub fn visible_branch(branch: Option<&str>) -> Option<&str> {
    branch.filter(|b| !b.is_empty() && *b != "HEAD")
}

#[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64 exactly")]
fn ms_of(iso: Option<&str>) -> f64 {
    iso.and_then(parse_date).map_or(0.0, |ms| ms as f64)
}

/// Epoch ms of the last activity, skipping a stamp more than `SKEW_MS`
/// ahead of `now`; 0 when none is left.
fn recent_key(h: &SessionHeader, now: f64) -> f64 {
    [ms_of(Some(&h.updated_at)), ms_of(h.started_at.as_deref())]
        .into_iter()
        .find(|&ms| ms > 0.0 && ms <= now + SKEW_MS)
        .unwrap_or(0.0)
}

/// Indexes of `headers`, newest activity first, ties by id.
pub fn sort_by_recent(headers: &[SessionHeader], now: f64) -> Vec<usize> {
    let keys: Vec<f64> = headers.iter().map(|h| recent_key(h, now)).collect();
    let mut order: Vec<usize> = (0..headers.len()).collect();
    order.sort_by(|&a, &b| {
        keys[b]
            .partial_cmp(&keys[a])
            .unwrap_or(Ordering::Equal)
            .then_with(|| locale_compare(&headers[a].id, &headers[b].id))
    });
    order
}

/// Whether a session's file changed between two listings; a session missing
/// from either counts as changed.
pub fn changed(old: Option<&SessionHeader>, new: Option<&SessionHeader>) -> bool {
    match (old, new) {
        (Some(a), Some(b)) => a.size_bytes != b.size_bytes || a.updated_at != b.updated_at,
        _ => true,
    }
}

/// The session wrote a line within `LIVE_MS` of `now`.
pub fn is_live(h: &SessionHeader, now: f64) -> bool {
    let key = recent_key(h, now);
    key > 0.0 && now - key < LIVE_MS
}

fn active_span(h: &SessionHeader) -> Option<(&str, &str)> {
    let start = day_of(h.started_at.as_deref());
    let end = day_of(Some(&h.updated_at));
    let from = start.or(end)?;
    let to = end.or(start).unwrap_or(from);
    Some((from, if to < from { from } else { to }))
}

/// Every day the session was active on, oldest first, at most 366.
pub fn active_days(h: &SessionHeader) -> Vec<String> {
    let Some((from, to)) = active_span(h) else {
        return Vec::new();
    };
    let mut days = vec![from.to_owned()];
    let mut ms = parse_date(from).unwrap_or(0);
    while days.last().is_some_and(|d| d.as_str() < to) && days.len() < MAX_SPAN_DAYS {
        ms += DAY_MS;
        days.push(day_key(ms));
    }
    days
}

fn in_range(h: &SessionHeader, range: &DayRange) -> bool {
    let Some((from, to)) = active_span(h) else {
        return false;
    };
    let pick_to = range.to.as_deref().unwrap_or(&range.from);
    from <= pick_to && to >= range.from.as_str()
}

/// The header passes every active filter and the title/id query.
pub fn matches(h: &SessionHeader, f: &Filters) -> bool {
    if f.agent.is_some_and(|a| a != h.agent)
        || f.project.as_deref().is_some_and(|p| p != project_key(h))
        || f.branch
            .as_deref()
            .is_some_and(|b| Some(b) != h.git_branch.as_deref())
        || f.model
            .as_deref()
            .is_some_and(|m| Some(m) != h.model.as_deref())
        || f.source
            .as_deref()
            .is_some_and(|s| s != h.source_label.as_deref().unwrap_or_default())
        || f.range.as_ref().is_some_and(|r| !in_range(h, r))
    {
        return false;
    }
    f.query.is_empty()
        || format!("{} {}", title_of(h), h.id)
            .to_lowercase()
            .contains(&f.query.to_lowercase())
}

/// The filters that hide `h`, each checked on its own.
pub fn hiding_filters(h: &SessionHeader, f: &Filters) -> Vec<FilterName> {
    FilterName::ALL
        .into_iter()
        .filter(|&n| !matches(h, &f.only(n)))
        .collect()
}

/// Indexes of `headers` that pass `f`, in the given `order`.
pub fn visible_rows(headers: &[SessionHeader], order: &[usize], f: &Filters) -> Vec<usize> {
    order
        .iter()
        .copied()
        .filter(|&i| matches(&headers[i], f))
        .collect()
}

/// Distinct agents present, in `AgentId::ALL` order.
pub fn agents_present(headers: &[SessionHeader]) -> Vec<AgentId> {
    AgentId::ALL
        .into_iter()
        .filter(|a| headers.iter().any(|h| h.agent == *a))
        .collect()
}

#[cfg(test)]
mod tests;
