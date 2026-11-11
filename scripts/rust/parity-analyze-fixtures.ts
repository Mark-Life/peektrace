/**
 * The analyze/export edge HOME for `parity.ts`: transcripts from
 * `parity-analyze-records.ts` plus context, memory and subagent files.
 * Paths depend on the fixture HOME, so the tree is built at run time.
 */
import { chmodSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import {
  asst,
  attachments,
  badTime,
  bomCrlf,
  compaction,
  cwdEmpty,
  doc,
  L,
  MEMORY_LINES,
  models,
  nonIsoTime,
  numbers,
  oddContent,
  queued,
  result,
  results,
  rewind,
  rewindDeadLeaf,
  server,
  snapshots,
  streamed,
  surrogate,
  text,
  thinking,
  tool,
  user,
  WIDE_MEMORY,
} from "./parity-analyze-records";

const NO_ACCESS = 0o000;

export const SECRETS = [
  "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA\n-----END RSA PRIVATE KEY-----",
  `key sk-ant-api03-${"A1b2".repeat(12)}`,
  `openai sk-proj-${"x9Y8".repeat(7)} and sk-ant-short`,
  `gh ghp_${"a1B2".repeat(9)} pat github_pat_${"c".repeat(82)}`,
  `gl glpat-${"d".repeat(20)} aws AKIA${"ABCD1234".repeat(2)}`,
  `google AIza${"e".repeat(35)} oauth ya29.${"f".repeat(25)}`,
  `slack xoxb-${"1".repeat(15)} app xapp-1-${"g".repeat(12)}`,
  `hook https://hooks.slack.com/services/T123/B456/${"h".repeat(24)}`,
  `stripe sk_live_${"i".repeat(20)} whsec_${"j".repeat(25)}`,
  `sg SG.${"k".repeat(22)}.${"l".repeat(43)} npm npm_${"m".repeat(36)}`,
  `do dop_v1_${"ab12".repeat(16)} hf hf_${"n".repeat(34)}`,
  `jwt eyJ${"o".repeat(12)}.${"p".repeat(8)}.${"q".repeat(10)}`,
  `Authorization: Bearer ${"r".repeat(25)}`,
  `"Authorization": "Basic ${"dXNlcjpwYXNz".repeat(2)}="`,
  "mongodb+srv://admin:pa55word@host/db and https://user@host/x",
  `aws_secret_access_key = "${"Ab1/Cd2+Ef3=Gh4/Ij5+Kl6=Mn7/Op8+Qr9=St0/"}"`,
  `twilio AC${"0a1b".repeat(8)} and SK${"2c3d".repeat(8)}`,
  `password: hunter2hunter2 token=abcdef123 api_key = 'placeholder' secret=\${SECRET}`,
  'export DB_PASSWORD=supersecret123\nAPI_URL=https://x\nMY_TOKEN="changeme"\r\nPRIVATE_KEY=abc\u2028NAME_KEY=zzz',
  "credential is Zx9Qw3Er5Ty7Ui9Op1As3Df5Gh7Jk9Lz2 and plain Zx9Qw3Er5Ty7Ui9Op1As3Df5Gh7Jk9Lz3",
  "webhook a1b2c3d4e5a1b2c3d4e5a1b2c3d4e5a1 blob iVBORw0KGgo1234567890abcdefghijklmnop",
  "\u017Fecret=abcdefgh and pwd=12345678 and auth_token: your-token-here",
];

const secrets = doc(
  { ...user(1, SECRETS.join("\n\n")), cwd: "/tmp/secret-dir" },
  { type: "ai-title", aiTitle: `title ghp_${"a1B2".repeat(9)}` },
  asst(2, "r1", [
    text(SECRETS.slice(0, 4).join(" ")),
    tool("t1", { k: SECRETS[12] }),
  ]),
  user(3, [result("t1", SECRETS[17])])
);

const homeForms = (home: string) => [
  "/Users/demo/proj and /home/alice/x and /var/home/bob/y",
  "file:///Users/demo/z and file:///home/u/w",
  "C:\\Users\\John Smith\\proj and C:/Users/jane/x and /mnt/c/Users/max/y and /c/users/zed/q",
  "\\\\wsl.localhost\\Ubuntu\\home\\neo\\src and \\\\wsl$\\Debian\\home\\trin",
  "-Users-demo-proj and C--Users-jo-x and --home-u-work-- and --home-dir",
  `${home}/project and ${home.replace(/[^a-zA-Z0-9]/g, "-")}-proj and --${home.slice(1).replace(/[\\/:]/g, "-")}-proj--`,
  `${home}x is not home, ${home}-y neither`,
];

const homes = (home: string) =>
  doc(
    { ...user(1, homeForms(home).join("\n")), cwd: `${home}/work/proj` },
    asst(2, "r1", [text(homeForms(home).join(" | "))]),
    { type: "ai-title", aiTitle: `Fix ${home}/thing` }
  );

const markdown = doc(
  user(1, "prompt with ```js\nunclosed"),
  asst(2, "r1", [text("answer ```\ncode")]),
  user(3, "inline ``` `a` ``` span"),
  asst(4, "r2", [text("text\n<!-- open comment")]),
  user(5, "<!-- closed --> fine"),
  asst(6, "r3", [text("~~~\ntilde fence")]),
  asst(7, "r4", [tool("t", { body: "a ```` b" })]),
  user(8, "specials & < > \" ' in html"),
  asst(9, "r5", [text("   \n  trailing ws  \n\n")])
);

/** Claude Code's project dir slug: every char outside `[a-zA-Z0-9]` becomes `-`. */
const claudeSlug = (p: string) => p.replace(/[^a-zA-Z0-9]/g, "-");
/** Older Claude Code slug: only `/` and `.` become `-`. */
const legacySlug = (p: string) => p.replace(/[/.]/g, "-");

const subagentMain = doc(
  user(1, "spawn"),
  asst(2, "r1", [tool("call-1", { prompt: "x" }, "Workflow")]),
  {
    ...user(3, [result("call-1", "started")]),
    toolUseResult: { runId: "wf_1" },
  },
  asst(4, "r2", [tool("call-2", { prompt: "y" }, "Workflow")]),
  {
    ...user(5, [result("call-2", "resumed")]),
    timestamp: "2026-03-01T10:00:30.000Z",
    toolUseResult: { runId: "wf_1" },
  },
  asst(6, "r3", [text("done")])
);

const sub = (n: number) =>
  doc(
    { ...user(n, "task"), isSidechain: true },
    { ...asst(n + 1, "s1", [text("sub work")]), isSidechain: true }
  );

/** Every file of the analyze edge HOME, keyed by path relative to it. */
const homeFiles = (home: string): Record<string, string> => {
  const proj = join(home, "work/proj");
  const agentsOnly = join(home, "work/agents-only");
  const spaced = join(home, "work/my_app dir");
  const legacy = join(home, "work/old_app");
  const projects = ".claude/projects/-edge";
  const subs = `${projects}/subs/subagents`;
  return {
    ".claude/CLAUDE.md": "# Global rules\nBe brief.\n",
    ".claude/rules/a.md": "Always test.\n",
    ".claude/rules/b.md": '---\npaths: src/**, "lib/{a,b}"\n---\nOnly src.\n',
    ".claude/rules/sub/c.md": "---\npaths:\n  - '**'\n---\nEverywhere.\n",
    ".claude/rules/notes.txt": "not a rule",
    "work/proj/.git/HEAD": "ref: refs/heads/main\n",
    "work/proj/CLAUDE.md": "Project rules\n",
    "work/proj/.claude/rules/r.md": "Project rule\n",
    "work/proj/CLAUDE.local.md": "Local\n",
    "work/proj/pkg/AGENTS.md": "ignored in a Claude project\n",
    "work/proj/.worktrees/wt/.git": `gitdir: ${proj}/.git/worktrees/wt\n`,
    "work/proj/.worktrees/wt/CLAUDE.md": "Worktree rules\n",
    "work/agents-only/.git/HEAD": "ref: refs/heads/main\n",
    "work/agents-only/AGENTS.md": "Agents rules\n",
    "work/agents-only/.claude/AGENTS.md": "More agents rules\n",
    "work/my_app dir/.git/HEAD": "ref: refs/heads/main\n",
    "work/old_app/.git/HEAD": "ref: refs/heads/main\n",
    [`.claude/projects/${claudeSlug(proj)}/memory/MEMORY.md`]: `\n${Array.from({ length: MEMORY_LINES }, (_, i) => `- note ${i} 😀`).join("\n")}\n`,
    [`.claude/projects/${claudeSlug(agentsOnly)}/memory/MEMORY.md`]: `${"w".repeat(WIDE_MEMORY - 10)}\n${"z".repeat(100)}`,
    [`.claude/projects/${claudeSlug(spaced)}/memory/MEMORY.md`]:
      "- spaced note\n",
    [`.claude/projects/${legacySlug(legacy)}/memory/MEMORY.md`]:
      "- legacy note\n",
    [`${projects}/rewind.jsonl`]: rewind,
    [`${projects}/rewind-dead-leaf.jsonl`]: rewindDeadLeaf,
    [`${projects}/compaction.jsonl`]: compaction,
    [`${projects}/snapshots.jsonl`]: snapshots,
    [`${projects}/queued.jsonl`]: queued,
    [`${projects}/attachments.jsonl`]: attachments,
    [`${projects}/results.jsonl`]: results,
    [`${projects}/thinking.jsonl`]: thinking,
    [`${projects}/streamed.jsonl`]: streamed,
    [`${projects}/models.jsonl`]: models,
    [`${projects}/odd-content.jsonl`]: oddContent,
    [`${projects}/server.jsonl`]: server,
    [`${projects}/numbers.jsonl`]: numbers,
    [`${projects}/surrogate.jsonl`]: surrogate,
    [`${projects}/bom-crlf.jsonl`]: bomCrlf,
    [`${projects}/cwd-empty.jsonl`]: cwdEmpty,
    [`${projects}/nonis-time.jsonl`]: nonIsoTime,
    [`${projects}/bad-time.jsonl`]: badTime,
    [`${projects}/secrets.jsonl`]: secrets,
    [`${projects}/homes.jsonl`]: homes(home),
    [`${projects}/markdown.jsonl`]: markdown,
    [`${projects}/context.jsonl`]: doc({
      ...user(1, "x"),
      cwd: join(proj, "pkg"),
    }),
    [`${projects}/context-wt.jsonl`]: doc({
      ...user(1, "x"),
      cwd: join(proj, ".worktrees/wt/src"),
    }),
    [`${projects}/context-agents.jsonl`]: doc({
      ...user(1, "x"),
      cwd: agentsOnly,
    }),
    [`${projects}/context-spaced.jsonl`]: doc({ ...user(1, "x"), cwd: spaced }),
    [`${projects}/context-legacy.jsonl`]: doc({ ...user(1, "x"), cwd: legacy }),
    [`${projects}/nullrow.jsonl`]: doc(
      user(1, "x"),
      "null",
      asst(2, "r1", [text("y")])
    ),
    [`${projects}/subs.jsonl`]: subagentMain,
    [`${subs}/agent-a.jsonl`]: sub(10),
    [`${subs}/agent-a.meta.json`]: L({
      agentType: "Explore",
      description: `look at ${home}/x with ghp_${"a1B2".repeat(9)}`,
      toolUseId: "call-1",
    }),
    [`${subs}/agent-b.jsonl`]: sub(11),
    [`${subs}/agent-b.meta.json`]: "{bad json",
    [`${subs}/agent-c.jsonl`]: sub(12),
    [`${subs}/nested/agent-d.jsonl`]: sub(13),
    [`${subs}/nested/agent-d.meta.json`]: L({
      parentAgentId: "a",
      workflowPhase: "solo",
    }),
    [`${subs}/workflows/wf_1/agent-w1.jsonl`]: sub(20),
    [`${subs}/workflows/wf_1/agent-w2.jsonl`]: sub(40),
    [`${subs}/workflows/wf_1/agent-w2.meta.json`]: L({ workflowPhase: "" }),
    [`${subs}/workflows/wf_2/agent-w3.jsonl`]: sub(50),
    [`${subs}/workflows/notes.txt`]: "x",
    [`${projects}/subs/workflows/wf_1.json`]: L({
      workflowProgress: [
        {
          type: "workflow_agent",
          agentId: "w1",
          phaseIndex: 0,
          phaseTitle: "Plan",
          startedAt: Date.parse("2026-03-01T10:00:10.000Z"),
        },
        { type: "workflow_agent", agentId: "agent-w1", phaseIndex: 9 },
        {
          type: "workflow_agent",
          agentId: "w2",
          phaseIndex: 1,
          phaseTitle: "Build",
        },
        { type: "other", agentId: "w3" },
      ],
    }),
  };
};

/** Files made unreadable; the caller restores them. */
export const LOCKED = [".claude/projects/-edge/subs/subagents/agent-c.jsonl"];

/** Build the analyze edge HOME under `home`. */
export const buildAnalyzeEdge = (home: string) => {
  for (const [rel, body] of Object.entries(homeFiles(home))) {
    const path = join(home, rel);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, body);
  }
  for (const rel of LOCKED) {
    chmodSync(join(home, rel), NO_ACCESS);
  }
};
