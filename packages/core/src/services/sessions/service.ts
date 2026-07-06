/** SessionsService: lazy listing, full parse, and context-budget analysis. */
import { FileSystem } from "@effect/platform";
import { Context, Effect, Layer, Option } from "effect";
import { AgentRegistry, type AgentRegistryShape } from "../agents";
import { analyze } from "./analyze";
import { type SessionNotFoundError, TranscriptParseError } from "./errors";
import { buildHeader } from "./header";
import { parseClaudeSession } from "./parse";
import { redactParsed, redactSession } from "./redact";
import {
  findSubagents,
  gatherOnDiskContextFiles,
  resolveClaudeSession,
  type SubagentStub,
} from "./resolve";
import type {
  AnalyzedSession,
  ParsedSession,
  SessionHeader,
  SubagentRef,
} from "./schema";

const LIST_CONCURRENCY = 8;
const JSONL = /\.jsonl$/;

const join = (...parts: readonly string[]): string =>
  parts.join("/").replace(/\/+/g, "/");

const mtimeMsOf = (info: FileSystem.File.Info): number =>
  Option.match(info.mtime, {
    onNone: () => 0,
    onSome: (d) => d.getTime(),
  });

/** Options accepted by `parse`. */
export interface ParseRequest {
  readonly id: string;
  /** Redact secret-looking transcript text. Default true. */
  readonly redact?: boolean;
}

/** Options accepted by `analyze`. */
export interface AnalyzeRequest {
  /** Dumb-zone threshold as a fraction of the window. Default 0.40. */
  readonly dumbZone?: number;
  readonly id: string;
  /** Redact secret-looking transcript text. Default true. */
  readonly redact?: boolean;
  /** Explicit context-window override (tokens). Default 1,000,000. */
  readonly window?: number;
}

/** Service contract for Claude session ingest + analysis. */
export interface SessionsServiceShape {
  /** Reproduce the context-budget forensics for one transcript. */
  readonly analyze: (
    req: AnalyzeRequest
  ) => Effect.Effect<
    AnalyzedSession,
    SessionNotFoundError | TranscriptParseError
  >;
  /** Lightweight headers for every Claude transcript, without a body parse. */
  readonly list: () => Effect.Effect<readonly SessionHeader[]>;
  /** Full parse of one transcript, folding in its subagent transcripts. */
  readonly parse: (
    req: ParseRequest
  ) => Effect.Effect<
    ParsedSession,
    SessionNotFoundError | TranscriptParseError
  >;
}

/** Claude session ingest + analysis. */
export class SessionsService extends Context.Tag("@peephole/SessionsService")<
  SessionsService,
  SessionsServiceShape
>() {}

/** Read a transcript file, mapping IO failures to TranscriptParseError. */
const readTranscript = (fs: FileSystem.FileSystem, path: string) =>
  fs
    .readFileString(path)
    .pipe(
      Effect.mapError(
        (e) => new TranscriptParseError({ path, reason: String(e) })
      )
    );

/** Parse one subagent file into a SubagentRef (its own window, sidechain on). */
const parseSubagent = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly stub: SubagentStub;
}): Effect.Effect<SubagentRef> => {
  const { fs, stub } = args;
  return fs.readFileString(stub.path).pipe(
    Effect.orElseSucceed(() => ""),
    Effect.map((text) => {
      const parsed = parseClaudeSession({
        text,
        path: stub.path,
        sessionId: stub.id,
        includeSidechainTurns: true,
      });
      const peakContextTokens = parsed.turns.reduce(
        (max, t) => Math.max(max, t.contextTokens),
        0
      );
      return {
        ...stub,
        turns: parsed.turns.length,
        peakContextTokens,
      } satisfies SubagentRef;
    })
  );
};

/** Full unredacted parse incl. folded subagents. */
const parseFull = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly id: string;
}): Effect.Effect<
  ParsedSession,
  SessionNotFoundError | TranscriptParseError
> => {
  const { fs, agents, id } = args;
  return Effect.gen(function* () {
    const resolved = yield* resolveClaudeSession({ fs, agents, idOrPath: id });
    const text = yield* readTranscript(fs, resolved.path);
    const parsed = parseClaudeSession({
      text,
      path: resolved.path,
      sessionId: resolved.sessionId,
    });
    const stubs = yield* findSubagents({
      fs,
      ...(resolved.subagentDir ? { subagentDir: resolved.subagentDir } : {}),
    });
    const subagents = yield* Effect.forEach(
      stubs,
      (stub) => parseSubagent({ fs, stub }),
      { concurrency: LIST_CONCURRENCY }
    );
    return { ...parsed, subagents } satisfies ParsedSession;
  }).pipe(Effect.withSpan("Sessions.parse", { attributes: { id } }));
};

/** List headers for every transcript file under one project slug. */
const listSlug = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly projectsRoot: string;
  readonly slug: string;
}): Effect.Effect<SessionHeader[]> => {
  const { fs, projectsRoot, slug } = args;
  const projectDir = join(projectsRoot, slug);
  return fs.readDirectory(projectDir).pipe(
    Effect.orElseSucceed(() => [] as string[]),
    Effect.flatMap((names) =>
      Effect.forEach(
        names.filter((n) => JSONL.test(n)),
        (name) => {
          const path = join(projectDir, name);
          return Effect.gen(function* () {
            const info = yield* fs.stat(path);
            if (info.type !== "File") {
              return null;
            }
            const text = yield* fs
              .readFileString(path)
              .pipe(Effect.orElseSucceed(() => ""));
            return buildHeader({
              text,
              id: name.replace(JSONL, ""),
              slug,
              path,
              sizeBytes: Number(info.size),
              mtimeMs: mtimeMsOf(info),
            });
          }).pipe(Effect.orElseSucceed(() => null));
        },
        { concurrency: LIST_CONCURRENCY }
      )
    ),
    Effect.map((headers) =>
      headers.filter((h): h is SessionHeader => h !== null)
    )
  );
};

const makeService = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
}): SessionsServiceShape => {
  const { fs, agents } = args;
  const projectsRoot = agents.roots("claude").projectsRoot;

  const list: SessionsServiceShape["list"] = () =>
    agents.listProjectSlugs("claude").pipe(
      Effect.orElseSucceed(() => [] as readonly string[]),
      Effect.flatMap((slugs) =>
        Effect.forEach(slugs, (slug) => listSlug({ fs, projectsRoot, slug }), {
          concurrency: LIST_CONCURRENCY,
        })
      ),
      Effect.map((groups) => groups.flat()),
      Effect.withSpan("Sessions.list")
    );

  const parse: SessionsServiceShape["parse"] = ({ id, redact = true }) =>
    parseFull({ fs, agents, id }).pipe(
      Effect.map((p) => (redact ? redactParsed(p) : p))
    );

  const analyzeReq: SessionsServiceShape["analyze"] = ({
    id,
    window,
    dumbZone,
    redact = true,
  }) =>
    Effect.gen(function* () {
      const parsed = yield* parseFull({ fs, agents, id });
      const onDiskContextFiles = yield* gatherOnDiskContextFiles({
        fs,
        agents,
        ...(parsed.cwd ? { cwd: parsed.cwd } : {}),
      });
      const result = analyze(parsed, {
        onDiskContextFiles,
        ...(window === undefined ? {} : { window }),
        ...(dumbZone === undefined ? {} : { dumbZoneFraction: dumbZone }),
      });
      return redact ? redactSession(result) : result;
    }).pipe(Effect.withSpan("Sessions.analyze", { attributes: { id } }));

  return { list, parse, analyze: analyzeReq };
};

/** Live layer: depends on AgentRegistry + the platform FileSystem. */
export const SessionsServiceLive = Layer.effect(
  SessionsService,
  Effect.gen(function* () {
    const fs = yield* FileSystem.FileSystem;
    const agents = yield* AgentRegistry;
    return makeService({ fs, agents });
  })
);
