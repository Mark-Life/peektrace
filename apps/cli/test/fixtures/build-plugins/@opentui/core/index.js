// Synthetic stand-in for @opentui/core: lazy tree-sitter assets, a worker, a native lib.
export const loadGrammar = () =>
  import("./assets/lang.wasm", { with: { type: "file" } });
export const loadQuery = () =>
  import("./assets/lang.scm", { with: { type: "file" } });
export const worker = await import("./parser.worker.js", {
  with: { type: "file" },
});
