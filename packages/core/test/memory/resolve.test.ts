import { describe, expect, test } from "bun:test";
import { labelSlugs } from "../../src/services/memory/resolve";

const labels = (slugs: readonly string[]) => [...labelSlugs(slugs).values()];

describe("labelSlugs", () => {
  test("no slugs gives no labels", () => {
    expect(labels([])).toEqual([]);
  });

  test("a single project keeps its whole slug, so dashed names stay whole", () => {
    expect(labels(["-home-dev-code-my-proj"])).toEqual([
      "home-dev-code-my-proj",
    ]);
  });

  test("strips the prefix at segment boundaries, not mid-word", () => {
    expect(labels(["-home-dev-code-app", "-home-dev-code-api"])).toEqual([
      "app",
      "api",
    ]);
    expect(
      labels(["-home-dev-code-peektrace", "-home-dev-code-peektrace-s3"])
    ).toEqual(["peektrace", "peektrace-s3"]);
  });

  test("a slug equal to the shared prefix keeps its last segment", () => {
    expect(labels(["-home-dev-code", "-home-dev-code-web"])).toEqual([
      "code",
      "code-web",
    ]);
  });

  test("slugs with nothing in common only lose the leading dash", () => {
    expect(labels(["-home-dev-a", "-srv-b"])).toEqual(["home-dev-a", "srv-b"]);
  });
});
