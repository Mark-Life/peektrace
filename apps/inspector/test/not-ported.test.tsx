/** Views whose API the server does not serve yet show a short note. */
import { describe, expect, test } from "bun:test";
import { Cause } from "effect";
import { renderToStaticMarkup } from "react-dom/server";
import { NotPorted } from "../src/components/not-ported";
import { isNotPorted } from "../src/lib/wire-error";

describe("not ported", () => {
  test("NotPortedYetError is recognised, other failures are not", () => {
    expect(
      isNotPorted(Cause.fail({ _tag: "NotPortedYetError", feature: "stats" }))
    ).toBe(true);
    expect(
      isNotPorted(
        Cause.fail({ _tag: "TransportError", message: "x", status: 501 })
      )
    ).toBe(false);
  });

  test("the note renders without a Retry button", () => {
    const html = renderToStaticMarkup(
      <NotPorted message="Stats are not available in the Rust build yet." />
    );
    expect(html).toContain('data-testid="not-ported"');
    expect(html).toContain("Stats are not available in the Rust build yet.");
    expect(html).not.toContain("Retry");
  });
});
