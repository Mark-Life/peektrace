/** Re-export of the shared tokenizer, which lives in `@workspace/core/highlight`
 * so the terminal UI can use the same one. Kept as a module path because the web
 * components import it as `@workspace/ui/lib/highlighter`.
 */
export * from "@workspace/core/highlight";
