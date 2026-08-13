/**
 * Spike harness for `.docs/spike/wire-capture.md` — a loopback-only capture
 * proxy that records what an agent actually sends to the model API, plus a
 * summarizer that decomposes one captured request into system / tools /
 * messages.
 *
 * This is investigation tooling, not a shipped Peektrace surface. It exists so
 * the numbers in the spike write-up can be re-derived on another machine.
 *
 *   # terminal 1
 *   bun run scripts/wire-capture.ts --out .wire-capture
 *   # terminal 2
 *   ANTHROPIC_BASE_URL=http://127.0.0.1:8899 claude -p "hello"
 *   # then
 *   bun run scripts/wire-capture.ts --summarize .wire-capture
 *
 * Capture files are redacted with core's `redactText` before they touch disk
 * unless `--no-redact` is passed. Credential headers are dropped either way.
 */
import { readdir } from "node:fs/promises";
import { redactText } from "../packages/core/src/services/sessions/redact";

const DEFAULT_PORT = 8899;
const DEFAULT_OUT = ".wire-capture";
const DEFAULT_UPSTREAM = "https://api.anthropic.com";
const IDLE_TIMEOUT_SECONDS = 120;
const ID_WIDTH = 3;
const CHARS_PER_TOKEN = 4;
const PREVIEW_CHARS = 90;
const NAME_COLUMN = 34;
const COUNT_COLUMN = 8;

/** Headers never written to a capture file, redaction on or off. */
const CREDENTIAL_HEADERS = new Set([
  "authorization",
  "cookie",
  "proxy-authorization",
  "x-api-key",
]);

const PATH_SEPARATORS = /\//g;

interface Options {
  readonly out: string;
  readonly port: number;
  readonly redact: boolean;
  readonly summarize: string | undefined;
  readonly upstream: string;
}

const parseArgs = (argv: readonly string[]): Options => {
  const flag = (name: string): string | undefined => {
    const i = argv.indexOf(`--${name}`);
    return i >= 0 ? argv[i + 1] : undefined;
  };
  return {
    out: flag("out") ?? DEFAULT_OUT,
    port: Number(flag("port") ?? process.env.PORT ?? DEFAULT_PORT),
    redact: !argv.includes("--no-redact"),
    summarize: flag("summarize"),
    upstream: flag("upstream") ?? DEFAULT_UPSTREAM,
  };
};

const estTokens = (chars: number): number =>
  Math.round(chars / CHARS_PER_TOKEN);

const parseJson = (text: string): unknown => {
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return text;
  }
};

/* ------------------------------------------------------------------ capture */

/** Header map minus anything that could authenticate as the user. */
const safeHeaders = (headers: Headers): Record<string, string> => {
  const out: Record<string, string> = {};
  for (const [key, value] of headers.entries()) {
    if (!CREDENTIAL_HEADERS.has(key.toLowerCase())) {
      out[key] = value;
    }
  }
  return out;
};

const writeCapture = async (args: {
  readonly body: string;
  readonly id: string;
  readonly opts: Options;
  readonly req: Request;
  readonly url: URL;
}): Promise<void> => {
  const { body, id, opts, req, url } = args;
  const record = {
    method: req.method,
    path: url.pathname + url.search,
    headers: safeHeaders(req.headers),
    body: body ? parseJson(body) : null,
  };
  const json = JSON.stringify(record, null, 2);
  const slug = url.pathname.replace(PATH_SEPARATORS, "_");
  await Bun.write(
    `${opts.out}/${id}${slug}.json`,
    opts.redact ? redactText(json) : json
  );
};

/** Forward one request upstream verbatim, streaming the response back. */
const forward = async (args: {
  readonly body: string | undefined;
  readonly req: Request;
  readonly target: string;
}): Promise<Response> => {
  const { body, req, target } = args;
  const headers = new Headers(req.headers);
  headers.delete("host");
  headers.delete("content-length");
  const res = await fetch(target, {
    method: req.method,
    headers,
    body,
    redirect: "manual",
  });
  // The upstream body is re-streamed as-is; length/encoding no longer apply.
  const out = new Headers(res.headers);
  out.delete("content-encoding");
  out.delete("content-length");
  return new Response(res.body, { status: res.status, headers: out });
};

const serve = (opts: Options): void => {
  let seq = 0;
  Bun.serve({
    port: opts.port,
    hostname: "127.0.0.1",
    idleTimeout: IDLE_TIMEOUT_SECONDS,
    fetch: async (req) => {
      const url = new URL(req.url);
      const hasBody = req.method !== "GET" && req.method !== "HEAD";
      const body = hasBody ? await req.text() : undefined;
      seq += 1;
      const id = String(seq).padStart(ID_WIDTH, "0");
      await writeCapture({ body: body ?? "", id, opts, req, url });
      process.stderr.write(
        `[${id}] ${req.method} ${url.pathname} ${body?.length ?? 0}B\n`
      );
      return await forward({
        body,
        req,
        target: opts.upstream + url.pathname + url.search,
      });
    },
  });
  process.stderr.write(
    `wire-capture on http://127.0.0.1:${opts.port} -> ${opts.upstream}\n` +
      `captures: ${opts.out}/  redaction: ${opts.redact ? "on" : "OFF"}\n` +
      `point an agent at it, e.g. ANTHROPIC_BASE_URL=http://127.0.0.1:${opts.port}\n`
  );
};

/* ---------------------------------------------------------------- summarize */

interface CachedBlock {
  readonly cache_control?: { readonly ttl?: string; readonly type: string };
  readonly text?: string;
  readonly type?: string;
}

interface MessagesBody {
  readonly messages?: readonly {
    readonly content?: readonly CachedBlock[] | string;
    readonly role?: string;
  }[];
  readonly model?: string;
  readonly system?: readonly CachedBlock[] | string;
  readonly tools?: readonly { readonly name: string }[];
}

const blockChars = (block: CachedBlock | string): number =>
  typeof block === "string" ? block.length : JSON.stringify(block).length;

const cacheMark = (block: CachedBlock): string =>
  block.cache_control ? ` [cache ${block.cache_control.ttl ?? "5m"}]` : "";

const row = (name: string, chars: number, extra = ""): string =>
  `  ${name.padEnd(NAME_COLUMN)}${String(estTokens(chars)).padStart(COUNT_COLUMN)} est tok${extra}`;

const summarizeSystem = (body: MessagesBody): readonly string[] => {
  const system = body.system;
  if (!system) {
    return ["  (no system)"];
  }
  if (typeof system === "string") {
    return [row("system", system.length)];
  }
  return system.map((block, i) =>
    row(
      `system[${i}]`,
      blockChars(block),
      `${cacheMark(block)}  ${JSON.stringify((block.text ?? "").slice(0, PREVIEW_CHARS))}`
    )
  );
};

const summarizeTools = (body: MessagesBody): readonly string[] => {
  const tools = body.tools ?? [];
  if (tools.length === 0) {
    return ["  (no tools)"];
  }
  const sized = tools
    .map((tool) => ({ name: tool.name, chars: JSON.stringify(tool).length }))
    .sort((a, b) => b.chars - a.chars);
  const total = sized.reduce((sum, t) => sum + t.chars, 0);
  return [
    row(`${tools.length} tool definitions`, total),
    ...sized.map((t) => row(`  ${t.name}`, t.chars)),
  ];
};

const summarizeMessages = (body: MessagesBody): readonly string[] =>
  (body.messages ?? []).map((message, i) => {
    const content = message.content;
    const blocks = Array.isArray(content) ? content : [content ?? ""];
    const chars = blocks.reduce<number>((sum, b) => sum + blockChars(b), 0);
    const marks = Array.isArray(content)
      ? content
          .map((b) => `${b.type ?? "?"}${cacheMark(b)}`)
          .join(" ")
          .slice(0, PREVIEW_CHARS)
      : "text";
    return row(`messages[${i}] ${message.role ?? "?"}`, chars, `  ${marks}`);
  });

const summarizeFile = async (path: string): Promise<void> => {
  const record = (await Bun.file(path).json()) as { body?: MessagesBody };
  const body = record.body;
  if (!body?.model) {
    return;
  }
  const total = JSON.stringify(body).length;
  const lines = [
    `\n=== ${path}  model=${body.model}  ${estTokens(total)} est tok total`,
    "system:",
    ...summarizeSystem(body),
    "tools:",
    ...summarizeTools(body),
    "messages:",
    ...summarizeMessages(body),
  ];
  process.stdout.write(`${lines.join("\n")}\n`);
};

const summarize = async (target: string): Promise<void> => {
  const stat = await Bun.file(target).exists();
  if (stat) {
    await summarizeFile(target);
    return;
  }
  const names = (await readdir(target)).filter((n) => n.endsWith(".json"));
  for (const name of names.sort()) {
    await summarizeFile(`${target}/${name}`);
  }
};

/* --------------------------------------------------------------------- main */

const opts = parseArgs(process.argv.slice(2));
if (opts.summarize) {
  await summarize(opts.summarize);
} else {
  serve(opts);
}
