import { describe, expect, test } from "bun:test";
import { tildePath } from "../src/lib/paths";

const HOME = "/home/dev";

describe("tildePath", () => {
  test("collapses the exact home dir", () => {
    expect(tildePath({ path: HOME, home: HOME })).toBe("~");
  });

  test("collapses a child path", () => {
    expect(tildePath({ path: `${HOME}/.claude/projects`, home: HOME })).toBe(
      "~/.claude/projects"
    );
  });

  test("leaves a sibling dir sharing the prefix", () => {
    expect(tildePath({ path: "/home/devx/.claude", home: HOME })).toBe(
      "/home/devx/.claude"
    );
  });

  test("leaves a path outside home", () => {
    expect(tildePath({ path: "/var/data/sessions", home: HOME })).toBe(
      "/var/data/sessions"
    );
  });

  test("tolerates a trailing slash on home and an empty home", () => {
    expect(tildePath({ path: `${HOME}/.pi`, home: `${HOME}/` })).toBe("~/.pi");
    expect(tildePath({ path: "/a/b", home: "" })).toBe("/a/b");
  });

  test("collapses Windows paths with backslash separators", () => {
    const home = "C:\\Users\\dev";
    expect(tildePath({ path: `${home}\\.claude\\projects`, home })).toBe(
      "~\\.claude\\projects"
    );
    expect(tildePath({ path: `${home}\\.pi`, home: `${home}\\` })).toBe(
      "~\\.pi"
    );
    expect(tildePath({ path: "C:\\Users\\devx", home })).toBe(
      "C:\\Users\\devx"
    );
  });
});
