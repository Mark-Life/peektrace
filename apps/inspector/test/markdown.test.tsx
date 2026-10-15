/** The transcript Markdown renderer, against hostile input.
 *
 * Transcripts carry text from tools, web pages and other agents, so the safety
 * claims in `markdown.tsx` are checked on the rendered markup itself.
 */
import { describe, expect, test } from "bun:test";
import {
  fenceLanguage,
  MARKDOWN_CHAR_LIMIT,
  Markdown,
  parseTranscriptMarkdown,
} from "@workspace/ui/components/ai-elements/markdown";
import { renderToStaticMarkup } from "react-dom/server";

const html = (text: string) => renderToStaticMarkup(<Markdown text={text} />);

describe("raw HTML", () => {
  test("is escaped, never injected", () => {
    const out = html(
      '<script>alert(1)</script>\n\n<img src=x onerror="alert(1)">\n\nhi <b onclick="x">b</b>'
    );
    expect(out).not.toContain("<script");
    expect(out).not.toContain("<img");
    expect(out).not.toContain("<b ");
    expect(out).toContain("&lt;script&gt;");
    expect(out).toContain("&lt;b onclick=");
  });

  test("tag-like harness markers stay readable text", () => {
    expect(html("<system-reminder>\nbe brief\n</system-reminder>")).toContain(
      "&lt;system-reminder&gt;"
    );
  });
});

describe("links", () => {
  test("executable and data URLs lose their link but keep the label", () => {
    for (const href of [
      "javascript:alert(1)",
      "JaVaScRiPt:alert(1)",
      "java\tscript:alert(1)",
      "data:text/html,<b>x</b>",
      "vbscript:msgbox(1)",
    ]) {
      const out = html(`[click](${href})`);
      expect(out).not.toContain("<a");
      expect(out).toContain("click");
    }
  });

  test("relative and in-page links do not navigate the hash-routed app", () => {
    expect(html("[a](#/settings) [b](./x.md)")).not.toContain("<a");
  });

  test("web links open in a new tab without an opener", () => {
    const out = html("[docs](https://example.com/a?b=1)");
    expect(out).toContain('href="https://example.com/a?b=1"');
    expect(out).toContain('target="_blank"');
    expect(out).toContain('rel="noopener noreferrer nofollow"');
  });

  test("images become links, so nothing is fetched", () => {
    const out = html("![chart](https://example.com/c.png)");
    expect(out).not.toContain("<img");
    expect(out).not.toContain("preload");
    expect(out).toContain("image: chart");
  });
});

describe("blocks", () => {
  test("a fenced block renders through the highlighted code block", () => {
    const out = html("Run:\n\n```ts\nconst a: number = 1;\n```");
    expect(out).toContain('data-markdown-fence="ts"');
    expect(out).toContain('data-language="typescript"');
    expect(out).toContain('class="th-');
    expect(out).toContain("const");
  });

  test("fences nested in lists and quotes are rewritten too", () => {
    const doc = parseTranscriptMarkdown(
      "- step\n\n  ```bash\n  ls\n  ```\n\n> ```json\n> {}\n> ```"
    );
    expect(JSON.stringify(doc)).not.toContain('"type":"code"');
    expect(JSON.stringify(doc)).toContain('"tagName":"md-fence"');
  });

  test("lists, emphasis and inline code render as elements", () => {
    const out = html("- **bold** and `x`\n- two");
    expect(out).toContain("<ul");
    expect(out).toContain("<strong>bold</strong>");
    expect(out).toContain("<code");
  });

  test("headings carry no id that could clobber the page's own", () => {
    expect(html("# Title")).not.toContain(" id=");
  });

  test("frontmatter stays visible as a code block, not silently dropped", () => {
    const out = html("---\nname: skill\n---\n\n# Body");
    expect(out).toContain("name: skill");
    expect(out).toContain('data-markdown-fence="yaml"');
    expect(out).toContain("<h1");
  });

  test("footnotes carry no element ids either", () => {
    expect(html("a[^1]\n\n[^1]: note")).not.toContain(" id=");
  });

  test("unknown fence languages fall back to plain text", () => {
    expect(fenceLanguage("python")).toBe("text");
    expect(fenceLanguage("TSX")).toBe("typescript");
    expect(fenceLanguage(undefined)).toBe("text");
  });

  test("text past the size limit renders verbatim, unparsed", () => {
    const big = `# heading\n${"x".repeat(MARKDOWN_CHAR_LIMIT)}`;
    const out = html(big);
    expect(out).not.toContain("<h1");
    expect(out).toContain("# heading");
  });
});
