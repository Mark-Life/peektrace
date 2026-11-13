/**
 * Fake credentials the parity fixtures plant to test redaction. The fixture
 * generators read them from here, and `parity-oracle.ts` stores each one in
 * goldens as `<fake-secret:name>`, so secret scanners never see a token in
 * the goldens. Built from parts so this file spells no whole token either.
 */
const join = (...parts: string[]) => parts.join("");

export const FAKE_SECRETS = {
  anthropic: join("sk-ant-", "api03-", "A1b2".repeat(12)),
  anthropicFixture: join(
    "sk-ant-",
    "api03-",
    "AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234"
  ),
  anthropicOpencode: join(
    "sk-ant-",
    "api03-",
    "AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHH1234"
  ),
  anthropicTelemetry: join("sk-ant-", "A".repeat(50)),
  awsKeyId: join("AKIA", "ABCD1234".repeat(2)),
  awsSecret: join("Ab1/Cd2+Ef3=Gh4/", "Ij5+Kl6=Mn7/Op8+Qr9=St0/"),
  digitalocean: join("dop_", "v1_", "ab12".repeat(16)),
  github: join("ghp_", "a1B2".repeat(9)),
  githubPat: join("github_", "pat_", "c".repeat(82)),
  githubTelemetry: join("ghp_", "abcdefghijklmnopqrstuvwxyz", "0123456789"),
  gitlab: join("glpat-", "d".repeat(20)),
  google: join("AIza", "e".repeat(35)),
  googleOauth: join("ya29.", "f".repeat(25)),
  huggingface: join("hf_", "n".repeat(34)),
  jwt: join("eyJ", "o".repeat(12), ".", "p".repeat(8), ".", "q".repeat(10)),
  jwtSigned: join(
    "eyJhbGciOiJIUzI1NiJ9.",
    "eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4ifQ.",
    "SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"
  ),
  mongodb: join("mongodb+srv://", "admin:pa55word@host"),
  npm: join("npm_", "m".repeat(36)),
  openai: join("sk-", "proj-", "x9Y8".repeat(7)),
  pemHeader: join("-----BEGIN RSA ", "PRIVATE KEY-----"),
  sendgrid: join("SG.", "k".repeat(22), ".", "l".repeat(43)),
  slackApp: join("xapp-", "1-", "g".repeat(12)),
  slackBot: join("xoxb-", "1".repeat(15)),
  slackHook: join("hooks.slack.com/services/", "T123/B456/", "h".repeat(24)),
  stripe: join("sk_", "live_", "i".repeat(20)),
  stripeHook: join("whsec_", "j".repeat(25)),
  twilioAccount: join("AC", "0a1b".repeat(8)),
  twilioKey: join("SK", "2c3d".repeat(8)),
} as const;

export type FakeSecret = keyof typeof FAKE_SECRETS;

/** `[placeholder, secret]`, longest secret first so a token inside another
 * is never split. */
export const FAKE_SECRET_PAIRS = (
  Object.entries(FAKE_SECRETS) as [FakeSecret, string][]
)
  .map(([name, secret]) => [`<fake-secret:${name}>`, secret] as const)
  .sort((a, b) => b[1].length - a[1].length);

/** Golden text with each fake secret replaced by its placeholder. */
export const hideFakeSecrets = (text: string) =>
  FAKE_SECRET_PAIRS.reduce(
    (acc, [token, secret]) => acc.replaceAll(secret, token),
    text
  );

/** `hideFakeSecrets` undone: the text TS really printed. */
export const revealFakeSecrets = (text: string) =>
  FAKE_SECRET_PAIRS.reduce(
    (acc, [token, secret]) => acc.replaceAll(token, secret),
    text
  );
