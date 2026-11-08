// Synthetic non-OpenTUI package that embeds its own wasm.
export const loadOther = () =>
  import("./other.wasm", { with: { type: "file" } });
