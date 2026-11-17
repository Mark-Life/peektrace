import { describe, expect, test } from "bun:test";

// The same cases the Rust `peektrace_protocol::marks` test reads, so the web
// and the native UIs draw the same strings.
import FIXTURE from "../../../tests/fixtures/session-marks.json";
import type { Provider, SessionHeader } from "../src/api-types.gen";
import {
  harnessMark,
  launcherMark,
  providerMark,
  projectName,
  sessionBranch,
  sessionModels,
  worktreeHint,
} from "../src/session-marks";

const NVIDIA = { id: "nvidia", name: "NVIDIA" };

describe("launcher", () => {
  test("hint names only the parts the launcher knows", () => {
    expect(launcherMark({ name: "t3code" }).title).toBe("Started from t3code");
    expect(
      launcherMark({ branch: "feat/a", name: "t3code", worktreePath: "/wt/a" })
        .title
    ).toBe("Started from t3code · branch feat/a · worktree /wt/a");
  });

  test("t3code draws T3, others their first two letters", () => {
    expect(launcherMark({ name: "t3code" }).text).toBe("T3");
    expect(launcherMark({ name: "zed" }).text).toBe("Ze");
    expect(launcherMark({ name: "t3code" }).label).toBe("Started from t3code");
  });
});

describe("harnessMark", () => {
  test("one monogram and full name per harness", () => {
    expect(harnessMark("claude-code")).toEqual({
      label: "Claude Code",
      text: "CC",
      title: "Claude Code",
    });
    expect(harnessMark("codex").text).toBe("Cx");
    expect(harnessMark("pi").text).toBe("Pi");
    expect(harnessMark("opencode").text).toBe("OC");
  });
});

describe("provider", () => {
  test("line names the maker and the exact model id", () => {
    expect(
      providerMark({ models: ["nvidia/GLM-5.3-Flash-NVFP4"], provider: NVIDIA })
        ?.label
    ).toBe("NVIDIA · nvidia/GLM-5.3-Flash-NVFP4");
    expect(
      providerMark({ models: ["mystery"], provider: undefined })?.label
    ).toBe("Unknown provider · mystery");
  });

  test("tooltip puts the main model first, then every other model", () => {
    expect(
      providerMark({
        models: ["nvidia/GLM-5.3-Flash-NVFP4", "moonshotai/Kimi-K3"],
        provider: NVIDIA,
      })?.title
    ).toBe("NVIDIA · nvidia/GLM-5.3-Flash-NVFP4\nmoonshotai/Kimi-K3");
  });

  test("an unknown maker draws a neutral mark; no model draws none", () => {
    expect(providerMark({ models: ["x-1"], provider: undefined })).toEqual({
      label: "Unknown provider · x-1",
      text: "?",
      title: "Unknown provider · x-1",
    });
    expect(providerMark({ models: [], provider: NVIDIA })).toBeUndefined();
  });

  test("models list the main model first, each once, no empty ids", () => {
    expect(sessionModels({ model: "b", models: ["a", "b", ""] })).toEqual([
      "b",
      "a",
    ]);
    expect(sessionModels({})).toEqual([]);
  });
});

describe("worktree", () => {
  const worktree = { path: "/h/.t3/worktrees/app/t3-1", project: "app" };

  test("hint names the path and the parent thread when known", () => {
    expect(worktreeHint({ parentTitle: "Plan it", worktree })).toBe(
      "Worktree: /h/.t3/worktrees/app/t3-1\nStarted by: Plan it"
    );
    expect(worktreeHint({ parentTitle: undefined, worktree })).toBe(
      "Worktree: /h/.t3/worktrees/app/t3-1"
    );
  });

  test("branch falls back to the worktree's for an empty or detached one", () => {
    const wt = { ...worktree, branch: "feat/x" };
    expect(sessionBranch({ gitBranch: "HEAD", worktree: wt })).toBe("feat/x");
    expect(sessionBranch({ worktree: wt })).toBe("feat/x");
    expect(sessionBranch({ gitBranch: "main", worktree: wt })).toBe("main");
    expect(sessionBranch({ gitBranch: "HEAD", worktree })).toBeUndefined();
  });
});

/** Optional header fields, writable so a case sets only what it has. */
type Fields<K extends keyof SessionHeader> = {
  -readonly [P in K]?: NonNullable<SessionHeader[P]>;
};

/** A project-label case: the slug, plus the cwd and worktree it has. */
type ProjectCase = Fields<"cwd" | "worktree"> & Pick<SessionHeader, "project">;

const PROVIDERS: readonly Provider[] = [
  "claude-code",
  "codex",
  "opencode",
  "pi",
];

describe("shared with the Rust UIs", () => {
  test("harness monograms and names", () => {
    for (const h of FIXTURE.harnesses) {
      const provider = PROVIDERS.find((p) => p === h.provider);
      expect(provider).toBeDefined();
      expect(
        provider === undefined ? undefined : harnessMark(provider)
      ).toEqual({
        label: h.name,
        text: h.mono,
        title: h.name,
      });
    }
  });

  test("launcher monograms", () => {
    for (const l of FIXTURE.launchers) {
      expect(launcherMark({ name: l.name }).text).toBe(l.mono);
    }
  });

  test("provider lines", () => {
    for (const p of FIXTURE.providerLines) {
      const provider =
        p.maker === null ? undefined : { id: "x", name: p.maker };

      expect(providerMark({ models: [p.model], provider })?.label).toBe(p.line);
    }
  });

  test("branches", () => {
    for (const b of FIXTURE.branches) {
      const h: Fields<"gitBranch" | "worktree"> = {};

      if (b.gitBranch !== null) {
        h.gitBranch = b.gitBranch;
      }

      if (b.worktreeBranch !== null) {
        h.worktree = { branch: b.worktreeBranch, path: "/wt", project: "p" };
      }

      expect(sessionBranch(h)).toBe(b.shown ?? undefined);
    }
  });

  test("project labels", () => {
    for (const p of FIXTURE.projectLabels) {
      const h: ProjectCase = { project: p.slug };

      if (p.cwd !== null) {
        h.cwd = p.cwd;
      }

      if (p.worktreeProject !== null) {
        h.worktree = { path: "/wt", project: p.worktreeProject };
      }

      expect(projectName(h)).toBe(p.label);
    }
  });

  test("worktree hints", () => {
    for (const w of FIXTURE.worktreeHints) {
      expect(
        worktreeHint({
          parentTitle: w.parentTitle ?? undefined,
          worktree: { path: w.path, project: "p" },
        })
      ).toBe(w.hint);
    }
  });
});
