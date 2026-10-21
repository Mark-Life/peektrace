/** Synthetic fact rows shared by the stats tests. Every column, nothing real. */
import type { FinishContext } from "../../src/services/stats/accumulate";
import {
  DEFAULT_ROW_LIMIT,
  DEFAULT_SAMPLE_CAP,
} from "../../src/services/stats/rank";
import { makeRedactor } from "../../src/services/stats/redact";
import type {
  BashFact,
  CorpusSummary,
  SessionFact,
  ToolFact,
} from "../../src/services/stats/schema";

export const redact = makeRedactor({ enabled: true });

export const EMPTY_CORPUS: CorpusSummary = {
  transcripts: 0,
  sessions: 0,
  projects: 0,
  toolCalls: 0,
  bashCalls: 0,
  bashSec: 0,
  toolSec: 0,
  humanWaitSec: 0,
  kinds: {},
  agents: {},
};

export const contextOf = (caveats: string[] = []): FinishContext => ({
  corpus: EMPTY_CORPUS,
  window: { days: 60, fromIso: "", toIso: "" },
  redact,
  limit: DEFAULT_ROW_LIMIT,
  sampleCap: DEFAULT_SAMPLE_CAP,
  caveat: (note) => {
    caveats.push(note);
  },
});

export const toolFact = (over: Partial<ToolFact>): ToolFact => ({
  row: "tool",
  sid: "s1",
  agent: "claude",
  project: "proj",
  kind: "main",
  parent: null,
  run: null,
  seq: 0,
  toolUseId: null,
  tool: "Read",
  ts: "2026-08-01T00:00:00.000Z",
  durSec: 1,
  err: false,
  failed: false,
  failReason: null,
  detector: null,
  benign: null,
  sig: null,
  arg: null,
  outChars: 0,
  side: false,
  req: null,
  ctxTok: null,
  cause: null,
  causeArg: null,
  focus: null,
  ...over,
});

export const bashFact = (over: Partial<BashFact>): BashFact => ({
  ...toolFact({}),
  row: "bash",
  tool: "Bash",
  cmd: "true",
  bin: "true",
  family: "true",
  job: "other",
  present: ["other"],
  wait: null,
  declaredDelaySec: 0,
  piped: false,
  bg: false,
  capped: false,
  ...over,
});

export const sessionFact = (over: Partial<SessionFact>): SessionFact => ({
  row: "session",
  sid: "s1",
  agent: "claude",
  project: "proj",
  kind: "main",
  parent: null,
  run: null,
  startedAt: "2026-08-01T00:00:00.000Z",
  endedAt: null,
  title: null,
  source: null,
  cwd: null,
  models: [],
  turns: 0,
  userMessages: 0,
  toolCalls: 0,
  inputTokens: 0,
  outputTokens: 0,
  cacheReadTokens: 0,
  cacheCreationTokens: 0,
  maxContextTokens: 0,
  firstTurnCacheCreation: 0,
  ...over,
});
