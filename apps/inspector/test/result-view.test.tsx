/** ResultView failure branch: optional Retry button and its busy state. */
import { describe, expect, test } from "bun:test";
import { Result } from "@effect-atom/atom-react";
import { Cause, Option } from "effect";
import { renderToStaticMarkup } from "react-dom/server";
import { ResultView } from "../src/lib/result-view";
import { withLastSuccess } from "../src/lib/server-info";

const failed = (waiting = false) =>
  Result.failure<string, string>(Cause.fail("boom"), { waiting });

const render = (args: {
  readonly result: Result.Result<string, string>;
  readonly onRetry?: () => void;
}) =>
  renderToStaticMarkup(
    <ResultView onRetry={args.onRetry} result={args.result}>
      {(value) => <p>{value}</p>}
    </ResultView>
  );

const noop = () => undefined;
const DISABLED = /<button[^>]*\sdisabled=""/;

describe("ResultView retry", () => {
  test("a failure with onRetry shows an enabled Retry button", () => {
    const html = render({ result: failed(), onRetry: noop });
    expect(html).toContain('data-testid="result-error"');
    expect(html).toContain('data-testid="result-retry"');
    expect(html).toContain("Retry");
    expect(html).not.toMatch(DISABLED);
  });

  test("a failure without onRetry has no Retry button", () => {
    const html = render({ result: failed() });
    expect(html).toContain('data-testid="result-error"');
    expect(html).not.toContain('data-testid="result-retry"');
  });

  test("a failure being refetched disables the button", () => {
    const html = render({ result: failed(true), onRetry: noop });
    expect(html).toMatch(DISABLED);
    expect(html).toContain("Retrying…");
  });

  test("a success renders the value and no Retry", () => {
    const html = render({ result: Result.success("ok"), onRetry: noop });
    expect(html).toBe("<p>ok</p>");
  });
});

describe("withLastSuccess", () => {
  test("a failed refetch over a success keeps rendering the success", () => {
    const refetchFailed = Result.failure<string, string>(Cause.fail("boom"), {
      previousSuccess: Option.some(Result.success("ok")),
    });
    expect(render({ result: withLastSuccess(refetchFailed) })).toBe(
      "<p>ok</p>"
    );
  });

  test("a failure with no earlier success still shows the error", () => {
    const html = render({ result: withLastSuccess(failed()), onRetry: noop });
    expect(html).toContain('data-testid="result-error"');
  });
});
