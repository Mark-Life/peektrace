import { describe, expect, test } from "bun:test";

import { tildePath } from "../src/lib/paths";

const HOME = "/home/dev";

describe("tildePath", () => {
  test("collapses the exact home dir", () => {
    expect(tildePath({ home: HOME, path: HOME })).toBe("~");
  });

  test("collapses a child path", () => {
    expect(tildePath({ home: HOME, path: `${HOME}/.claude/projects` })).toBe(
      "~/.claude/projects"
    );
  });

  test("leaves a sibling dir sharing the prefix", () => {
    expect(tildePath({ home: HOME, path: "/home/devx/.claude" })).toBe(
      "/home/devx/.claude"
    );
  });

  test("leaves a path outside home", () => {
    expect(tildePath({ home: HOME, path: "/var/data/sessions" })).toBe(
      "/var/data/sessions"
    );
  });

  test("tolerates a trailing slash on home and an empty home", () => {
    expect(tildePath({ home: `${HOME}/`, path: `${HOME}/.pi` })).toBe("~/.pi");
    expect(tildePath({ home: "", path: "/a/b" })).toBe("/a/b");
  });

  test("collapses Windows paths with backslash separators", () => {
    const home = "C:\\Users\\dev";
    expect(tildePath({ home, path: `${home}\\.claude\\projects` })).toBe(
      "~\\.claude\\projects"
    );
    expect(tildePath({ home: `${home}\\`, path: `${home}\\.pi` })).toBe(
      "~\\.pi"
    );
    expect(tildePath({ home, path: "C:\\Users\\devx" })).toBe(
      "C:\\Users\\devx"
    );
  });
});
