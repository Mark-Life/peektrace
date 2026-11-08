/** Session list filters live in the hash, so opening a row or going Back keeps them. */
import { beforeEach, expect, test } from "bun:test";

const loc = { hash: "" };
let replaces = 0;
const win = {
  location: loc,
  history: {
    replaceState: (_: unknown, __: string, hash: string) => {
      replaces += 1;
      loc.hash = hash;
    },
  },
  addEventListener: () => undefined,
  removeEventListener: () => undefined,
  dispatchEvent: () => true,
};
Object.assign(globalThis, { window: win });

const {
  closeSession,
  markedSessionHash,
  navigate,
  openSession,
  openSessionMarked,
  openSubagent,
  setHashParam,
  setHashParams,
} = await import("../src/lib/routes");

const params = () => new URLSearchParams(loc.hash.split("?")[1] ?? "");

beforeEach(() => {
  loc.hash = "#/sessions?q=alpha&agent=codex";
  replaces = 0;
});

test("opening a session keeps the list filters in the hash", () => {
  openSession("id-0001");
  expect(params().get("id")).toBe("id-0001");
  expect(params().get("q")).toBe("alpha");
  expect(params().get("agent")).toBe("codex");
});

test("closing a session keeps the list filters in the hash", () => {
  openSession("id-0001");
  closeSession();
  expect(params().get("id")).toBeNull();
  expect(params().get("q")).toBe("alpha");
});

test("closing a session drops the transcript params", () => {
  loc.hash =
    "#/sessions?q=alpha&id=id-0002&parent=id-0001&mark=retry&call=c-1&view=chat";
  closeSession();
  expect(loc.hash).toBe("#/sessions?q=alpha");
});

test("leaving for another section and coming back restores the filters", () => {
  openSession("id-0001");
  navigate("stats");
  expect(loc.hash).toBe("#/stats");
  navigate("sessions");
  expect(params().get("q")).toBe("alpha");
  expect(params().get("agent")).toBe("codex");
  expect(params().get("id")).toBeNull();
});

test("a subagent opens with the list filters kept", () => {
  openSubagent({ id: "id-0002", parent: "id-0001" });
  expect(params().get("id")).toBe("id-0002");
  expect(params().get("parent")).toBe("id-0001");
  expect(params().get("q")).toBe("alpha");
});

test("a stats row link carries no list filters", () => {
  const hash = markedSessionHash({
    id: "id-0001",
    parent: null,
    mark: "retry",
  });
  const p = new URLSearchParams(hash.split("?")[1]);
  expect(p.get("q")).toBeNull();
  expect(p.get("agent")).toBeNull();
});

test("setHashParams writes every change in one replace", () => {
  setHashParams({ q: null, agent: null, model: "m-1", from: "2026-01-02" });
  expect(replaces).toBe(1);
  expect(loc.hash).toBe("#/sessions?model=m-1&from=2026-01-02");
});

test("an unchanged hash is not written again", () => {
  setHashParam("q", "alpha");
  setHashParams({ branch: null });
  expect(replaces).toBe(0);
});

test("opening a session from stats keeps the remembered list filters", () => {
  navigate("stats");
  openSessionMarked({ id: "id-0001", parent: null, mark: "retry" });
  expect(params().get("id")).toBe("id-0001");
  expect(params().get("mark")).toBe("retry");
  expect(params().get("q")).toBe("alpha");
  navigate("stats");
  navigate("sessions");
  expect(params().get("q")).toBe("alpha");
  expect(params().get("agent")).toBe("codex");
});
