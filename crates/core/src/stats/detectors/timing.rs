//! Time: which job a command runs (tests, build, waiting, ...) and the time
//! table charged per job.

use std::sync::LazyLock;

use indexmap::IndexMap;

use super::waiting::{compare_drill_hits, is_waiting, mask_quoted, strip_comments, strip_heredoc_bodies};
use crate::js::json_out::Writer;
use crate::js::regex::{LazyRe, Re};
use crate::js::text;
use crate::js_re;
use crate::stats::accumulate::{
    DrillHit, DrillTable, DrillTarget, FinishCtx, SampleRef, Tally, hit_of, redact_hits,
    sample_line, sample_of, samples_of, write_samples,
};
use crate::stats::rank::{cmp_str, desc, median, percentile, rank, round, share};
use crate::stats::schema::{Fact, Job};

const P95: f64 = 0.95;
const CEILING_SEC: f64 = 590.0;
const DEFAULT_TIMEOUT_LO: f64 = 118.0;
const DEFAULT_TIMEOUT_HI: f64 = 124.0;
const MAX_STRIP_PASSES: usize = 4;

static HEREDOC: LazyRe = js_re!(r#"<<-?\s*['"]?([A-Za-z_][A-Za-z0-9_]*)['"]?[\s\S]*?(?:\n\1|$)"#, "g");
static SPLIT: LazyRe = js_re!(r"&&|\|\||[;|\n]");
static LEAD: LazyRe = js_re!(
    r#"^\s*(?:\(|\{|!|do\b|then\b|else\b|nohup\b|exec\b|sudo\b|time\b|command\b|timeout\s+\d+m?\b|npx\b|bunx\b|bun\s+x\b|pnpm\s+dlx\b|npm\s+exec\b|xargs\b(?:\s+-\S+)*|--[\w-]+\b|[A-Za-z_][A-Za-z0-9_]*=(?:"[^"]*"|'[^']*'|\S*)\s+)+"#
);
static CD: LazyRe = js_re!(r#"^\s*cd\s+(?:"[^"]*"|'[^']*'|\S+)\s*"#);
static BASENAME: LazyRe = js_re!(r#"^["']?(?:[^\s"']*\/)+([\w.@+-]+)["']?"#);
static QUOTED_PATH: LazyRe = js_re!(r#"^"([^"]*\/)([^"/]+)""#);

/// The command split into classifiable segments, wrappers stripped.
pub fn segments(cmd: &str) -> Vec<String> {
    let stripped = if cmd.contains("<<") {
        HEREDOC.replace_all_str(cmd, " <<HEREDOC ")
    } else {
        cmd.to_owned()
    };
    SPLIT
        .split(&stripped)
        .into_iter()
        .map(|raw| {
            let mut seg = text::trim(raw).to_owned();
            for _ in 0..MAX_STRIP_PASSES {
                let a = LEAD.replace_first(&seg, |_| String::new());
                let b = CD.replace_first(&a, |_| String::new());
                let next = text::trim(&b).to_owned();
                if next == seg {
                    break;
                }
                seg = next;
            }
            let s = QUOTED_PATH.replace_first(&seg, |c| c.get(2).unwrap_or("").to_owned());
            BASENAME.replace_first(&s, |c| c.get(1).unwrap_or("").to_owned())
        })
        .filter(|s| !s.is_empty())
        .collect()
}

fn runner(script: &str) -> String {
    format!(r"^(?:bun|pnpm|npm|yarn|turbo|deno)\s+(?:run\s+|run-script\s+)?(?:[\w@/.-]*:)?{script}\b")
}

static SEGMENT_RULES: LazyLock<Vec<(Job, Re)>> = LazyLock::new(|| {
    let rules: Vec<(Job, String)> = vec![
        (Job::Test, r"^(?:vitest|jest|playwright\s+test|ava|mocha|pytest|py\.test|tap|bats)\b".into()),
        (Job::Test, r"^python3?\s+-m\s+(?:pytest|unittest)\b".into()),
        (Job::Test, r"^(?:cargo|go|gradle|mvn|dotnet)\s+test\b".into()),
        (Job::Test, runner(r"test(?:[:\w-]*)")),
        (Job::Test, r"^(?:bun|deno)\s+test\b".into()),
        (Job::Build, r"^(?:next|vite|tsup|esbuild|rollup|webpack|parcel|astro|remix|nuxt)\s+build\b".into()),
        (Job::Build, runner(r"build(?:[:\w-]*)")),
        (Job::Build, r"^(?:cargo\s+build|go\s+build|make\b|docker\s+build|docker\s+compose\s+build|gradle\s+build|mvn\s+package)".into()),
        (Job::Build, r"^tsc\b.*\s-b\b".into()),
        (Job::Typecheck, r"^(?:tsc|tsgo|effect-tsgo|vue-tsc|svelte-check)\b".into()),
        (Job::Typecheck, runner("(?:typecheck|type-check|tsc|types|check-types)")),
        (Job::Typecheck, r"^(?:mypy|pyright)\b".into()),
        (Job::Lint, r"^(?:biome|ultracite|eslint|prettier|oxlint|ruff|black|rustfmt|gofmt|dprint)\b".into()),
        (Job::Lint, runner(r"(?:lint|format|fmt)(?:[:\w-]*)")),
        (Job::Lint, r"^cargo\s+(?:clippy|fmt)\b".into()),
        (Job::PackageInstall, r"^(?:bun|pnpm|npm|yarn)\s+(?:install|i|add|ci|link|update|upgrade|dedupe|prune)\b".into()),
        (Job::PackageInstall, r"^(?:pip3?\s+install|uv\s+(?:pip\s+)?(?:add|sync|install)|cargo\s+(?:add|install)|brew\s+(?:install|upgrade)|corepack\b|go\s+get)\b".into()),
        (Job::Net, r"^(?:curl|wget|http|https|xh)\b".into()),
        (Job::ScriptRun, r"^(?:python3?|node|deno\s+run|tsx|ts-node|ruby|perl|php)\b".into()),
        (Job::ScriptRun, r"^bun\s+(?:run\s+)?[\w./-]*\.(?:ts|tsx|js|mjs|cjs)\b".into()),
        (Job::ScriptRun, r"^(?:bun|node|deno)\s+-e\b".into()),
        (Job::ScriptRun, r"^(?:bash|sh|zsh|\.\/|[\w.-]+\.sh\b)".into()),
        (Job::ScriptRun, r"^[\w.-]+\.(?:mjs|cjs|js|ts|py|rb)\b".into()),
        (Job::ScriptRun, r"^(?:agent-browser|chromium|(?:Google\s+)?Chrome\b|google-chrome|playwright(?!\s+test))".into()),
        (Job::ScriptRun, r"^(?:docker|podman|psql|sqlite3|redis-cli|kubectl)\b".into()),
        (Job::ScriptRun, r"^(?:bun|pnpm|npm|yarn|turbo)\s+run\s+\S".into()),
        (Job::Git, r"^git\b".into()),
        (Job::Git, r"^gh\b".into()),
        (Job::Git, r"^(?:jj|hg|svn)\b".into()),
        (Job::Search, r"^(?:grep|egrep|fgrep|rg|ag|ack|ugrep)\b".into()),
        (Job::Search, r"^(?:find|fd|mdfind|locate|which|type|whereis)\b".into()),
        (Job::FileIo, r"^(?:cat|bat|head|tail|less|more|sed|awk|cut|tr|sort|uniq|wc|tee|jq|yq|cp|mv|rm|mkdir|rmdir|touch|chmod|chown|ln|ls|tree|du|df|stat|diff|patch|realpath|basename|dirname|readlink|xxd|strings|base64|gzip|gunzip|tar|unzip|zip|split|paste|join|column|pbcopy|pbpaste|open)\b".into()),
        (Job::FileIo, r"^(?:echo|printf|true|false|:)\b".into()),
    ];
    rules.into_iter().map(|(j, src)| (j, Re::new(&src, ""))).collect()
});

pub const fn job_label(job: Job) -> &'static str {
    match job {
        Job::WaitPoll => "waiting",
        Job::Test => "tests",
        Job::Build => "build",
        Job::Typecheck => "typecheck",
        Job::Lint => "lint / format",
        Job::PackageInstall => "package install",
        Job::Net => "network fetch",
        Job::ScriptRun => "ad-hoc script",
        Job::Git => "git",
        Job::Search => "search",
        Job::FileIo => "file i/o",
        Job::Other => "other",
    }
}

/// The job a command is charged to, and every job it touches, in priority order.
pub fn classify_job(cmd: &str) -> (Job, Vec<Job>) {
    let mut present = [false; 12];
    let idx = |j: Job| Job::ALL.iter().position(|x| *x == j).unwrap_or(11);
    if is_waiting(cmd) {
        present[idx(Job::WaitPoll)] = true;
    }
    for seg in segments(cmd) {
        if let Some((job, _)) = SEGMENT_RULES.iter().find(|(_, re)| re.is_match(&seg)) {
            present[idx(*job)] = true;
        }
    }
    let ordered: Vec<Job> = Job::ALL
        .iter()
        .copied()
        .filter(|j| present[idx(*j)])
        .collect();
    let job = ordered.first().copied().unwrap_or(Job::Other);
    let present = if ordered.is_empty() { vec![job] } else { ordered };
    (job, present)
}

static BARE_AMP: LazyRe = js_re!(r"(?<![&>\\])&(?![&>])");

/// A lone `&` outside quotes, comments and heredoc bodies.
pub fn is_backgrounded(cmd: &str) -> bool {
    cmd.contains('&') && BARE_AMP.is_match(&mask_quoted(&strip_comments(&strip_heredoc_bodies(cmd))))
}

/// A duration at a harness ceiling: a floor, not a measurement.
pub fn is_capped(dur_sec: Option<f64>) -> bool {
    dur_sec.is_some_and(|d| d >= CEILING_SEC || (DEFAULT_TIMEOUT_LO..=DEFAULT_TIMEOUT_HI).contains(&d))
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimeRow {
    pub key: Job,
    pub label: &'static str,
    pub runs: f64,
    pub co_present_runs: f64,
    pub total_sec: f64,
    pub share: f64,
    pub median_sec: f64,
    pub p95_sec: f64,
    pub capped_runs: f64,
    pub capped_sec: f64,
    pub bg_runs: f64,
    pub bg_sec: f64,
    pub samples: Vec<SampleRef>,
}

impl TimeRow {
    pub fn write(&self, w: &mut Writer) {
        w.begin_obj();
        w.key("key").str(self.key.as_str());
        w.key("label").str(self.label);
        for (k, v) in [
            ("runs", self.runs),
            ("coPresentRuns", self.co_present_runs),
            ("totalSec", self.total_sec),
            ("share", self.share),
            ("medianSec", self.median_sec),
            ("p95Sec", self.p95_sec),
            ("cappedRuns", self.capped_runs),
            ("cappedSec", self.capped_sec),
            ("bgRuns", self.bg_runs),
            ("bgSec", self.bg_sec),
        ] {
            w.key(k).num(v);
        }
        w.key("samples");
        write_samples(w, &self.samples);
        w.end_obj();
    }
}

#[derive(Default, Clone, Copy)]
struct JobExtra {
    bg_runs: f64,
    bg_sec: f64,
    capped_runs: f64,
    capped_sec: f64,
    co_present_runs: f64,
}

#[derive(Default)]
pub struct TimeTable {
    wanted: Option<String>,
    tallies: IndexMap<Job, Tally>,
    extras: IndexMap<Job, JobExtra>,
    hits: Vec<DrillHit>,
}

impl TimeTable {
    pub fn new(drill: Option<&DrillTarget>) -> Self {
        Self {
            wanted: drill
                .filter(|d| d.table == DrillTable::Time)
                .map(|d| d.key.clone()),
            ..Self::default()
        }
    }

    pub fn add(&mut self, fact: &Fact) {
        let Fact::Call(f) = fact else { return };
        let Some(b) = f.bash.as_deref() else { return };
        let line = sample_line(&b.cmd);
        self.tallies
            .entry(b.job)
            .or_default()
            .add(f, Some(sample_of(f, line.clone())));
        let sec = f.dur_sec.unwrap_or(0.0);
        let extra = self.extras.entry(b.job).or_default();
        if b.capped {
            extra.capped_runs += 1.0;
            extra.capped_sec += sec;
        }
        if b.bg {
            extra.bg_runs += 1.0;
            extra.bg_sec += sec;
        }
        for job in &b.present {
            self.extras.entry(*job).or_default().co_present_runs += 1.0;
        }
        if self.wanted.as_deref() == Some(b.job.as_str()) {
            self.hits.push(hit_of(f, line));
        }
    }

    pub fn finish(&self, ctx: &mut FinishCtx<'_>) -> Vec<TimeRow> {
        let rows: Vec<TimeRow> = Job::ALL
            .iter()
            .filter_map(|&key| {
                let t = self.tallies.get(&key).filter(|t| t.rows > 0.0)?;
                let x = self.extras.get(&key).copied().unwrap_or_default();
                Some(TimeRow {
                    key,
                    label: job_label(key),
                    runs: t.rows,
                    co_present_runs: x.co_present_runs,
                    total_sec: round(t.total_sec),
                    share: share(t.total_sec, ctx.corpus.bash_sec),
                    median_sec: round(median(&t.durations)),
                    p95_sec: round(percentile(&t.durations, P95)),
                    capped_runs: x.capped_runs,
                    capped_sec: round(x.capped_sec),
                    bg_runs: x.bg_runs,
                    bg_sec: round(x.bg_sec),
                    samples: samples_of(&t.samples, ctx),
                })
            })
            .collect();
        let any_capped = rows.iter().any(|r| r.capped_runs > 0.0);
        let ranked = rank(
            rows,
            |a, b| {
                desc(a.total_sec, b.total_sec)
                    .then_with(|| desc(a.runs, b.runs))
                    .then_with(|| cmp_str(a.key.as_str(), b.key.as_str()))
            },
            ctx.limit,
        );
        if ranked.dropped > 0 {
            ctx.caveat(format!("time: {} job rows not shown", ranked.dropped));
        }
        if any_capped {
            ctx.caveat("a run that returned at a harness ceiling measures the ceiling, not the command: subtract cappedSec for the net total".to_owned());
        }
        ranked.rows
    }

    pub fn drill(&self, ctx: &FinishCtx<'_>) -> Vec<DrillHit> {
        let mut hits = redact_hits(&self.hits, ctx.redact);
        hits.sort_by(compare_drill_hits);
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs() {
        assert_eq!(classify_job("bun run test").0, Job::Test);
        assert_eq!(classify_job("cd x && git status").0, Job::Git);
        assert_eq!(classify_job("sleep 5 && bun test").1, vec![Job::WaitPoll, Job::Test]);
        assert_eq!(classify_job("weird-thing").1, vec![Job::Other]);
        assert!(is_backgrounded("bun dev &"));
        assert!(!is_backgrounded("a 2>&1 && b"));
    }
}
