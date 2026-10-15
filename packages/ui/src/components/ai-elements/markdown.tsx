"use client";

/** Markdown for untrusted transcript text, on `@tanstack/markdown`.
 *
 * Raw HTML stays literal text (`allowHtml` is never set), links keep only
 * `http(s)`/`mailto` targets and open in a new tab, images render as links so a
 * transcript cannot make the page fetch anything, and heading ids are off so
 * content cannot clobber the page's own element ids. Fenced code renders through
 * the shared `CodeBlock`, so it gets the same highlighter and copy button as
 * every other code pane.
 */

import type {
  BlockNode,
  ComponentNode,
  MarkdownDocument,
  UrlTransform,
} from "@tanstack/markdown";
import { parseMarkdown } from "@tanstack/markdown/parser";
import {
  type MarkdownComponentProps,
  type MarkdownComponents,
  renderMarkdownReact,
} from "@tanstack/markdown/react";
import type { CodeBlockLanguage } from "@workspace/ui/lib/highlighter";
import { cn } from "@workspace/ui/lib/utils";
import { memo, useMemo } from "react";
import { CodeBlock, CodeBlockCopyButton } from "./code-block";

/** Past this many characters the text renders as plain wrapped text: parsing
 *  stays fast, but a huge React tree per body does not. */
export const MARKDOWN_CHAR_LIMIT = 200_000;

const SAFE_URL = /^(?:https?:|mailto:)/i;

/** Drops every link target that is not an absolute web or mail address. A
 *  dropped link keeps its label as plain text. */
export const safeUrl: UrlTransform = (_url, _kind, defaultUrl) =>
  SAFE_URL.test(defaultUrl) ? defaultUrl : null;

const FENCE_LANGUAGES: Record<string, CodeBlockLanguage> = {
  bash: "bash",
  console: "bash",
  cjs: "typescript",
  javascript: "typescript",
  js: "typescript",
  json: "json",
  json5: "json",
  jsonc: "json",
  jsx: "typescript",
  markdown: "markdown",
  md: "markdown",
  mjs: "typescript",
  sh: "bash",
  shell: "bash",
  ts: "typescript",
  tsx: "typescript",
  typescript: "typescript",
  zsh: "bash",
};

/** The highlighter language for a fence's info string; unknown ones stay text. */
export const fenceLanguage = (lang: string | undefined): CodeBlockLanguage =>
  FENCE_LANGUAGES[lang?.toLowerCase() ?? ""] ?? "text";

/** Custom tag a fenced block is rewritten to, so React renders our `CodeBlock`. */
const FENCE_TAG = "md-fence";

/** Swaps every fenced block, at any depth, for a component node carrying the
 *  raw code — the React renderer only hands components props, not AST nodes. */
const withFences = (node: BlockNode): BlockNode => {
  switch (node.type) {
    case "code":
      return {
        attributes: {},
        children: [],
        name: "fence",
        properties: { code: node.value, lang: node.lang ?? "" },
        tagName: FENCE_TAG,
        type: "component",
      } satisfies ComponentNode;
    case "list":
      return {
        ...node,
        items: node.items.map((item) => ({
          ...item,
          children: item.children.map(withFences),
        })),
      };
    case "blockquote":
    case "callout":
    case "component":
      return { ...node, children: node.children.map(withFences) };
    case "footnotes":
      return {
        ...node,
        items: node.items.map((item) => ({
          ...item,
          children: item.children.map(withFences),
        })),
      };
    default:
      return node;
  }
};

/** Parses untrusted text with the safe options, fences rewritten. */
export const parseTranscriptMarkdown = (text: string): MarkdownDocument => {
  const doc = parseMarkdown(text, {
    headingIds: false,
    urlTransform: safeUrl,
  });
  return { ...doc, children: doc.children.map(withFences) };
};

/** Parsed documents by source text. Bodies remount on every view switch and
 *  filter change; re-parsing them each time is wasted work. */
const CACHE_LIMIT = 1000;
const cache = new Map<string, MarkdownDocument>();

const parseCached = (text: string) => {
  const hit = cache.get(text);
  if (hit) {
    return hit;
  }
  const doc = parseTranscriptMarkdown(text);
  if (cache.size >= CACHE_LIMIT) {
    const oldest = cache.keys().next().value;
    if (oldest !== undefined) {
      cache.delete(oldest);
    }
  }
  cache.set(text, doc);
  return doc;
};

const Fence = ({ code, lang }: { code: string; lang: string }) => (
  <CodeBlock
    className="my-2 [&_code]:text-xs! [&_pre]:p-3! [&_pre]:text-xs! [&_pre]:leading-relaxed!"
    code={code}
    data-markdown-fence={lang}
    language={fenceLanguage(lang)}
    wrap
  >
    <CodeBlockCopyButton className="absolute top-1 right-1 z-10 size-7" />
  </CodeBlock>
);

const Link = ({ children, href, title }: MarkdownComponentProps<"a">) =>
  href && SAFE_URL.test(href) ? (
    <a
      className="text-primary underline underline-offset-2 hover:opacity-80"
      href={href}
      rel="noopener noreferrer nofollow"
      target="_blank"
      title={title}
    >
      {children}
    </a>
  ) : (
    // Footnote and other in-page links: the app routes on the hash, so they
    // must not navigate.
    <span className="text-primary">{children}</span>
  );

/** An image as a link to it, never an `<img>`: no request leaves the page. */
const Image = ({ alt, src }: MarkdownComponentProps<"img">) => {
  const label = `image: ${alt || src || ""}`;
  return typeof src === "string" && SAFE_URL.test(src) ? (
    <Link href={src}>{label}</Link>
  ) : (
    <span className="text-muted-foreground">{label}</span>
  );
};

const BLOCK = "my-2 first:mt-0 last:mb-0";
const HEADING = cn(BLOCK, "font-semibold leading-snug");

const COMPONENTS: MarkdownComponents = {
  a: Link,
  [FENCE_TAG]: Fence,
  blockquote: ({ children }: MarkdownComponentProps<"blockquote">) => (
    <blockquote
      className={cn(BLOCK, "border-border border-l-2 pl-3 text-muted-foreground")}
    >
      {children}
    </blockquote>
  ),
  code: ({ children }: MarkdownComponentProps<"code">) => (
    <code className="wrap-anywhere rounded bg-muted px-1 py-px font-mono text-[0.9em]">
      {children}
    </code>
  ),
  h1: ({ children }: MarkdownComponentProps<"h1">) => (
    <h1 className={cn(HEADING, "text-[1.2em]")}>{children}</h1>
  ),
  h2: ({ children }: MarkdownComponentProps<"h2">) => (
    <h2 className={cn(HEADING, "text-[1.12em]")}>{children}</h2>
  ),
  h3: ({ children }: MarkdownComponentProps<"h3">) => (
    <h3 className={cn(HEADING, "text-[1.05em]")}>{children}</h3>
  ),
  h4: ({ children }: MarkdownComponentProps<"h4">) => (
    <h4 className={HEADING}>{children}</h4>
  ),
  h5: ({ children }: MarkdownComponentProps<"h5">) => (
    <h5 className={HEADING}>{children}</h5>
  ),
  h6: ({ children }: MarkdownComponentProps<"h6">) => (
    <h6 className={cn(HEADING, "text-muted-foreground")}>{children}</h6>
  ),
  hr: () => <hr className={cn(BLOCK, "border-border")} />,
  img: Image,
  li: ({ children }: MarkdownComponentProps<"li">) => (
    <li className="my-0.5 whitespace-pre-line">{children}</li>
  ),
  ol: ({ children, start }: MarkdownComponentProps<"ol">) => (
    <ol className={cn(BLOCK, "list-decimal pl-5")} start={start}>
      {children}
    </ol>
  ),
  p: ({ children }: MarkdownComponentProps<"p">) => (
    <p className={cn(BLOCK, "whitespace-pre-line")}>{children}</p>
  ),
  table: ({ children }: MarkdownComponentProps<"table">) => (
    <div className={cn(BLOCK, "max-w-full overflow-x-auto")}>
      <table className="border-collapse text-[0.95em]">{children}</table>
    </div>
  ),
  td: ({ children, style }: MarkdownComponentProps<"td">) => (
    <td className="border border-border px-2 py-1 align-top" style={style}>
      {children}
    </td>
  ),
  th: ({ children, style }: MarkdownComponentProps<"th">) => (
    <th
      className="border border-border bg-muted/50 px-2 py-1 text-left font-semibold"
      style={style}
    >
      {children}
    </th>
  ),
  ul: ({ children }: MarkdownComponentProps<"ul">) => (
    <ul className={cn(BLOCK, "list-disc pl-5")}>{children}</ul>
  ),
};

const RENDER_OPTIONS = { components: COMPONENTS };

export interface MarkdownProps {
  readonly className?: string;
  readonly text: string;
}

const MarkdownBody = ({ className, text }: MarkdownProps) => {
  const tooLarge = text.length > MARKDOWN_CHAR_LIMIT;
  const nodes = useMemo(
    () =>
      tooLarge ? null : renderMarkdownReact(parseCached(text), RENDER_OPTIONS),
    [text, tooLarge]
  );
  return (
    <div
      className={cn(
        "min-w-0 wrap-break-word",
        tooLarge && "whitespace-pre-wrap",
        className
      )}
      data-testid="markdown"
    >
      {nodes ?? text}
    </div>
  );
};

/** Rendered Markdown, re-parsed only when the text changes. */
export const Markdown = memo(MarkdownBody);
