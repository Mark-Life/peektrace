/**
 * Best-effort secret redaction for transcript-derived text before it leaves
 * core. Specific labeled rules run first and win; entropy fallbacks run last
 * and only fire adjacent to a credential keyword. All regexes are linear-time.
 */
import type {
  AnalyzedSession,
  ParsedSession,
  SubagentRef,
  TimelineEvent,
} from "./schema";

const ENTROPY_AWS = 4.0;
const ENTROPY_B64 = 3.5;
const ENTROPY_HEX = 3.0;
const CRED_LOOKBACK = 48;
const MAX_FALLBACK_LEN = 512;

/** Shannon entropy (bits/char) over a string's own characters. */
const entropy = (s: string): number => {
  const freq = new Map<string, number>();
  for (const ch of s) {
    freq.set(ch, (freq.get(ch) ?? 0) + 1);
  }
  let h = 0;
  for (const n of freq.values()) {
    const p = n / s.length;
    h -= p * Math.log2(p);
  }
  return h;
};

/** Env-var refs, booleans, and obvious dummy/example values never redacted. */
const PLACEHOLDER =
  /^(?:null|none|true|false|undefined|changeme|change-me|example|examples?|sample|placeholder|redacted|dummy|fixme|todo|foo|bar|baz|value|your[-_a-z0-9]*|my[-_a-z0-9]*|test|xxx+|\*+|<[^>]+>|\$\{?[A-Za-z0-9_]+\}?|process\.env(?:\.[A-Z_]+)?|os\.environ.*)$/i;

const DIGITS_ONLY = /^\d+$/;
const TWILIO = /twilio/i;
const QUOTE_TRIM = /^["']|["']$/g;
const HAS_DIGIT = /[0-9]/;
const HAS_ALPHA = /[A-Za-z]/;
const HAS_HEX_ALPHA = /[a-fA-F]/;
const ENV_SENSITIVE =
  /^(?:KEY|KEYS|TOKEN|TOKENS|SECRET|SECRETS|PASSWORD|PASSWD|PWD|PASS|CRED|CREDS|CREDENTIAL|CREDENTIALS|APIKEY|PRIVATE|PASSPHRASE|SIGNINGKEY)$/;
const ENV_DESCRIPTIVE =
  /(?:VERSION|URL|URI|HOST|PORT|PATH|DIR|NAME|ENABLED|DISABLED|MODE|ENV|REGION|BASE|ENDPOINT|PROVIDER|TYPE|PUBLIC|TIMEOUT|COUNT|LEVEL|FORMAT|PREFIX|SUFFIX)$/;

/** True for values that look like indirections/dummies rather than real secrets. */
const isPlaceholder = (v: string): boolean =>
  PLACEHOLDER.test(v) || DIGITS_ONLY.test(v) || v.startsWith("[REDACTED");

/** base64 payload magic prefixes / data-URIs — skip these as binary blobs. */
const BLOB_MAGIC =
  /^(?:iVBORw0KGgo|\/9j\/|JVBERi0|UEsDB|R0lGOD|UklGR|H4sI|AAAA|data:)/;

/** Credential-context keywords used to gate the entropy fallbacks. */
const CRED_CONTEXT =
  /(?:secret|token|api[_-]?key|apikey|access[_-]?key|private[_-]?key|password|passwd|\bpwd\b|credential|client[_-]?secret|signing|webhook|\bauth|bearer)/i;

/**
 * True when a credential keyword appears in the chars immediately before a
 * fallback match. `rest` is the trailing replacer args `[...groups, offset, string]`.
 */
const credContextAt = (rest: unknown[]): boolean => {
  const src = rest.at(-1);
  const offset = rest.at(-2);
  if (typeof src !== "string" || typeof offset !== "number") {
    return false;
  }
  return CRED_CONTEXT.test(
    src.slice(Math.max(0, offset - CRED_LOOKBACK), offset)
  );
};

/**
 * A redaction rule. `to` is either a static replacement string (supporting
 * $1..$n) or a function replacer used when context-sensitive guards are needed.
 */
interface Rule {
  readonly re: RegExp;
  readonly to: string | ((match: string, ...rest: string[]) => string);
}

const RULES: readonly Rule[] = [
  {
    re: /-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?PRIVATE KEY-----[\s\S]*?-----END (?:RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?PRIVATE KEY-----/g,
    to: "[REDACTED:private-key-block]",
  },
  { re: /sk-ant-[A-Za-z0-9_-]{40,}/g, to: "[REDACTED:anthropic-key]" },
  {
    re: /\bsk-(?!ant-)(?:proj-|svcacct-|admin-)?[A-Za-z0-9_-]{20,}/g,
    to: "[REDACTED:openai-key]",
  },
  {
    re: /\b(?:gh[opusr]_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{82})\b/g,
    to: "[REDACTED:github-token]",
  },
  { re: /\bglpat-[A-Za-z0-9_-]{20}\b/g, to: "[REDACTED:gitlab-token]" },
  {
    re: /\b(?:AKIA|ASIA|AGPA|AIDA|AROA|AIPA|ANPA|ABIA|ACCA)[A-Z0-9]{16}\b/g,
    to: "[REDACTED:aws-access-key-id]",
  },
  { re: /\bAIza[A-Za-z0-9_-]{35}\b/g, to: "[REDACTED:google-api-key]" },
  { re: /\bya29\.[A-Za-z0-9_-]{20,}/g, to: "[REDACTED:google-oauth-token]" },
  { re: /\bxox[baprse]-[A-Za-z0-9-]{10,48}\b/g, to: "[REDACTED:slack-token]" },
  { re: /\bxapp-[0-9]-[A-Za-z0-9-]{10,}\b/g, to: "[REDACTED:slack-token]" },
  {
    re: /https:\/\/hooks\.slack\.com\/services\/T[A-Za-z0-9]+\/B[A-Za-z0-9]+\/[A-Za-z0-9]{24}/g,
    to: "[REDACTED:slack-webhook]",
  },
  {
    re: /\b(?:sk|rk|pk)_(?:live|test)_[A-Za-z0-9]{10,99}\b/g,
    to: "[REDACTED:stripe-key]",
  },
  { re: /\bwhsec_[A-Za-z0-9]{20,}\b/g, to: "[REDACTED:stripe-webhook-secret]" },
  {
    re: /\bSG\.[A-Za-z0-9_-]{22}\.[A-Za-z0-9_-]{43}\b/g,
    to: "[REDACTED:sendgrid-key]",
  },
  { re: /\bnpm_[A-Za-z0-9]{36}\b/g, to: "[REDACTED:npm-token]" },
  { re: /\bdop_v1_[a-f0-9]{64}\b/g, to: "[REDACTED:digitalocean-token]" },
  { re: /\bhf_[A-Za-z0-9]{30,}\b/g, to: "[REDACTED:huggingface-token]" },
  {
    re: /\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{6,}\.[A-Za-z0-9_-]*\b/g,
    to: "[REDACTED:jwt]",
  },
  {
    re: /\b(Bearer\s+)[A-Za-z0-9._~+/-]{20,}={0,2}/g,
    to: "$1[REDACTED:bearer-token]",
  },
  {
    re: /(Authorization"?\s*:\s*"?Basic\s+)[A-Za-z0-9+/]{16,}={0,2}/gi,
    to: "$1[REDACTED:basic-credentials]",
  },
  {
    re: /\b(https?|postgres(?:ql)?|mysql|mongodb(?:\+srv)?|rediss?|amqps?|ftp):\/\/([^\s/:@]+):[^\s/@]+@/gi,
    to: "$1://$2:[REDACTED]@",
  },
  {
    re: /((?:aws|secret|access)[^\n]{0,40}?["'])([A-Za-z0-9/+=]{40})(["'])/gi,
    to: (m, g1, g2, g3) =>
      entropy(g2) >= ENTROPY_AWS ? `${g1}[REDACTED:aws-secret-key]${g3}` : m,
  },
  {
    re: /\b(?:AC|SK)[0-9a-f]{32}\b/g,
    to: (m, ...rest) => {
      const src = rest.at(-1);
      return typeof src === "string" && TWILIO.test(src)
        ? "[REDACTED:twilio-id]"
        : m;
    },
  },
  {
    re: /(password|passwd|pwd|secret|api[_-]?key|apikey|access[_-]?token|client[_-]?secret|auth[_-]?token|token)(\s*[:=]\s*)["']?([^\s"']{6,})["']?/gi,
    to: (m, g1, g2, g3) =>
      isPlaceholder(g3) ? m : `${g1}${g2}[REDACTED:credential]`,
  },
  {
    re: /^[ \t]*(?:export[ \t]+)?([A-Z][A-Z0-9_]*)([ \t]*=[ \t]*)(.+)$/gm,
    to: (m, key: string, sep: string, val: string) => {
      const sensitive = key.split("_").some((s) => ENV_SENSITIVE.test(s));
      const descriptive = ENV_DESCRIPTIVE.test(key);
      if (!sensitive || descriptive) {
        return m;
      }
      return isPlaceholder(val.replace(QUOTE_TRIM, ""))
        ? m
        : `${key}${sep}[REDACTED:env-secret]`;
    },
  },
  {
    re: /[A-Za-z0-9_-]{32,}={0,2}/g,
    to: (m, ...rest) => {
      if (m.length > MAX_FALLBACK_LEN || BLOB_MAGIC.test(m)) {
        return m;
      }
      if (!(HAS_DIGIT.test(m) && HAS_ALPHA.test(m))) {
        return m;
      }
      if (isPlaceholder(m) || !credContextAt(rest)) {
        return m;
      }
      return entropy(m) >= ENTROPY_B64 ? "[REDACTED:high-entropy]" : m;
    },
  },
  {
    re: /\b[0-9a-fA-F]{32,}\b/g,
    to: (m, ...rest) => {
      if (!(HAS_HEX_ALPHA.test(m) && HAS_DIGIT.test(m))) {
        return m;
      }
      if (!credContextAt(rest)) {
        return m;
      }
      return entropy(m) >= ENTROPY_HEX ? "[REDACTED:high-entropy-hex]" : m;
    },
  },
];

/**
 * Redact secret-looking substrings from a single string, best-effort.
 * Specific labeled rules win; entropy fallbacks fire only near a credential keyword.
 */
export const redactText = (s: string): string => {
  if (!s) {
    return s;
  }
  let out = s;
  for (const { re, to } of RULES) {
    out =
      typeof to === "string"
        ? out.replace(re, to)
        : out.replace(re, to as (m: string, ...rest: string[]) => string);
  }
  return out;
};

/** Redact every untrusted, transcript-derived string field of one event. */
const redactEvent = (e: TimelineEvent): TimelineEvent => ({
  ...e,
  title: e.title ? redactText(e.title) : e.title,
  preview: e.preview ? redactText(e.preview) : e.preview,
  body: e.body ? redactText(e.body) : e.body,
});

/** Redact every transcript-derived subagent description. */
const redactSubagents = (subs: readonly SubagentRef[]): SubagentRef[] =>
  subs.map((s) =>
    s.description ? { ...s, description: redactText(s.description) } : s
  );

/**
 * Return a copy of a parsed session with every transcript-derived string field
 * redacted, leaving numeric estimates and structure untouched.
 */
export const redactParsed = (p: ParsedSession): ParsedSession => ({
  ...p,
  ...(p.title ? { title: redactText(p.title) } : {}),
  events: p.events.map(redactEvent),
  subagents: redactSubagents(p.subagents),
});

/**
 * Return a copy of `a` with every transcript-derived string field redacted,
 * leaving numeric estimates and structure untouched.
 */
export const redactSession = (a: AnalyzedSession): AnalyzedSession => ({
  ...a,
  ...(a.title ? { title: redactText(a.title) } : {}),
  events: a.events.map(redactEvent),
  biggestItems: a.biggestItems.map(redactEvent),
  subagents: redactSubagents(a.subagents),
});
