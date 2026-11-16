//! Fact rows (one shard line each) and the enums the report is keyed by.

use crate::js::json_out::Writer;
use crate::model::Agent;

pub const STATS_SCHEMA_VERSION: &str = "peektrace-stats/v2";
pub const DETECTOR_VERSION: f64 = 2.0;
/// Part of the cache key: shards from another extractor are re-extracted.
pub const EXTRACTOR_VERSION: f64 = 13.0;

macro_rules! str_enum {
    ($(#[$doc:meta])* $name:ident { $($var:ident = $s:literal),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name { $($var),+ }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$var),+];

            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$var => $s),+ }
            }

            pub fn parse(s: &str) -> Option<Self> {
                match s { $($s => Some(Self::$var),)+ _ => None }
            }
        }
    };
}

str_enum!(
    /// Where a transcript sits in the session tree.
    Kind { Main = "main", Subagent = "subagent", WorkflowAgent = "workflow-agent" }
);

str_enum!(
    /// Intent group a command is charged to, in priority order.
    Job {
        WaitPoll = "wait/poll",
        Test = "test",
        Build = "build",
        Typecheck = "typecheck",
        Lint = "lint",
        PackageInstall = "package-install",
        Net = "net",
        ScriptRun = "script-run",
        Git = "git",
        Search = "search",
        FileIo = "file-io",
        Other = "other",
    }
);

str_enum!(
    /// How a command parks.
    WaitShape { LoopPoll = "loop-poll", Watcher = "watcher", CurlRetry = "curl-retry", BareDelay = "bare-delay" }
);

str_enum!(
    /// A named cause of a failed call, in catalog order.
    CauseId {
        ZshGlobFlag = "zsh-glob-flag",
        ZshGlobPath = "zsh-glob-path",
        ZshEqualsAbort = "zsh-equals-abort",
        PkillSelfKill = "pkill-self-kill",
        SleepBlocked = "sleep-blocked",
        CommandNotFound = "command-not-found",
        OutputTooLarge = "output-too-large",
        ReadBeforeWrite = "read-before-write",
        StaleRead = "stale-read",
        EditNoMatch = "edit-no-match",
        StructuredOutputSchema = "structured-output-schema",
        RmGuard = "rm-guard",
        ScratchModuleResolution = "scratch-module-resolution",
        UgrepComplexity = "ugrep-complexity",
        Unclassified = "unclassified",
    }
);

/// Columns every fact row carries.
#[derive(Debug, Clone, PartialEq)]
pub struct FactBase {
    pub sid: String,
    pub agent: Agent,
    pub project: String,
    pub kind: Kind,
    pub parent: Option<String>,
    pub run: Option<String>,
}

/// One transcript, summarised.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionFact {
    pub base: FactBase,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub source: Option<String>,
    pub models: Vec<String>,
    pub turns: f64,
    pub user_messages: f64,
    pub tool_calls: f64,
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub max_context_tokens: f64,
    pub first_turn_cache_creation: f64,
}

/// The shell columns of a bash call.
#[derive(Debug, Clone, PartialEq)]
pub struct BashPart {
    pub cmd: String,
    pub ran: Vec<String>,
    pub bin: String,
    pub family: String,
    pub job: Job,
    pub present: Vec<Job>,
    pub wait: Option<WaitShape>,
    pub declared_delay_sec: f64,
    pub piped: bool,
    pub bg: bool,
    pub capped: bool,
}

/// One tool call joined to its result; `bash` is set for shell calls.
#[derive(Debug, Clone, PartialEq)]
pub struct CallFact {
    pub base: FactBase,
    pub seq: f64,
    pub tool_use_id: Option<String>,
    pub tool: String,
    pub ts: Option<String>,
    pub dur_sec: Option<f64>,
    pub err: bool,
    pub failed: bool,
    pub fail_reason: Option<String>,
    pub detector: Option<String>,
    pub benign: Option<String>,
    pub sig: Option<String>,
    pub arg: Option<String>,
    pub out_chars: f64,
    pub side: bool,
    pub req: Option<String>,
    pub ctx_tok: Option<f64>,
    pub cause: Option<CauseId>,
    pub cause_arg: Option<String>,
    pub focus: Option<String>,
    pub bash: Option<Box<BashPart>>,
}

impl CallFact {
    pub fn is_bash(&self) -> bool {
        self.bash.is_some()
    }
}

/// One model call's token counts.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageFact {
    pub sid: String,
    pub agent: Agent,
    pub ts: Option<String>,
    pub req: String,
    pub model: String,
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Fact {
    Session(SessionFact),
    Call(CallFact),
    Usage(UsageFact),
}

impl Fact {
    pub fn as_call(&self) -> Option<&CallFact> {
        match self {
            Self::Call(c) => Some(c),
            _ => None,
        }
    }
}

/// `Writer` helpers for nullable values.
pub trait WriteExt {
    fn opt_text(&mut self, v: Option<&str>);
    fn opt_num(&mut self, v: Option<f64>);
}

impl WriteExt for Writer {
    fn opt_text(&mut self, v: Option<&str>) {
        match v {
            Some(s) => self.text(s),
            None => self.null(),
        }
    }

    fn opt_num(&mut self, v: Option<f64>) {
        match v {
            Some(n) => self.num(n),
            None => self.null(),
        }
    }
}

fn write_base(w: &mut Writer, b: &FactBase) {
    w.key("sid").text(&b.sid);
    w.key("agent").str(b.agent.as_str());
    w.key("project").text(&b.project);
    w.key("kind").str(b.kind.as_str());
    w.key("parent").opt_text(b.parent.as_deref());
    w.key("run").opt_text(b.run.as_deref());
}

fn write_session(w: &mut Writer, f: &SessionFact) {
    w.key("row").str("session");
    write_base(w, &f.base);
    w.key("startedAt").opt_text(f.started_at.as_deref());
    w.key("endedAt").opt_text(f.ended_at.as_deref());
    w.key("title").opt_text(f.title.as_deref());
    w.key("cwd").opt_text(f.cwd.as_deref());
    w.key("source").opt_text(f.source.as_deref());
    w.key("models").texts(f.models.iter().map(String::as_str));
    for (k, v) in [
        ("turns", f.turns),
        ("userMessages", f.user_messages),
        ("toolCalls", f.tool_calls),
        ("inputTokens", f.input_tokens),
        ("outputTokens", f.output_tokens),
        ("cacheReadTokens", f.cache_read_tokens),
        ("cacheCreationTokens", f.cache_creation_tokens),
        ("maxContextTokens", f.max_context_tokens),
        ("firstTurnCacheCreation", f.first_turn_cache_creation),
    ] {
        w.key(k).num(v);
    }
}

fn write_call(w: &mut Writer, f: &CallFact) {
    if f.bash.is_none() {
        w.key("row").str("tool");
    }
    write_base(w, &f.base);
    w.key("seq").num(f.seq);
    w.key("toolUseId").opt_text(f.tool_use_id.as_deref());
    w.key("tool").text(&f.tool);
    w.key("ts").opt_text(f.ts.as_deref());
    w.key("durSec").opt_num(f.dur_sec);
    w.key("err").bool(f.err);
    w.key("failed").bool(f.failed);
    w.key("failReason").opt_text(f.fail_reason.as_deref());
    w.key("detector").opt_text(f.detector.as_deref());
    w.key("benign").opt_text(f.benign.as_deref());
    w.key("sig").opt_text(f.sig.as_deref());
    w.key("arg").opt_text(f.arg.as_deref());
    w.key("outChars").num(f.out_chars);
    w.key("side").bool(f.side);
    w.key("req").opt_text(f.req.as_deref());
    w.key("ctxTok").opt_num(f.ctx_tok);
    w.key("cause").opt_text(f.cause.map(CauseId::as_str));
    w.key("causeArg").opt_text(f.cause_arg.as_deref());
    w.key("focus").opt_text(f.focus.as_deref());
    if let Some(b) = &f.bash {
        w.key("row").str("bash");
        w.key("cmd").text(&b.cmd);
        w.key("ran").texts(b.ran.iter().map(String::as_str));
        w.key("bin").text(&b.bin);
        w.key("family").text(&b.family);
        w.key("job").str(b.job.as_str());
        w.key("present").texts(b.present.iter().map(|j| j.as_str()));
        w.key("wait").opt_text(b.wait.map(WaitShape::as_str));
        w.key("declaredDelaySec").num(b.declared_delay_sec);
        w.key("piped").bool(b.piped);
        w.key("bg").bool(b.bg);
        w.key("capped").bool(b.capped);
    }
}

fn write_usage(w: &mut Writer, f: &UsageFact) {
    w.key("row").str("usage");
    w.key("sid").text(&f.sid);
    w.key("agent").str(f.agent.as_str());
    w.key("ts").opt_text(f.ts.as_deref());
    w.key("req").text(&f.req);
    w.key("model").text(&f.model);
    w.key("inputTokens").num(f.input_tokens);
    w.key("outputTokens").num(f.output_tokens);
    w.key("cacheReadTokens").num(f.cache_read_tokens);
    w.key("cacheCreationTokens").num(f.cache_creation_tokens);
}

/// `JSON.stringify(fact)`.
pub fn fact_json(f: &Fact) -> String {
    let mut w = Writer::new(false);
    w.begin_obj();
    match f {
        Fact::Session(s) => write_session(&mut w, s),
        Fact::Call(c) => write_call(&mut w, c),
        Fact::Usage(u) => write_usage(&mut w, u),
    }
    w.end_obj();
    w.finish()
}
