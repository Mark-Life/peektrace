# Spike: TanStack Charts for `packages/viz`

**Date:** 2026-08-08 · **Versions measured:** `@tanstack/charts` / `@tanstack/react-charts` / `@tanstack/charts-scales` `0.7.2`, `recharts` `3.8.0`

**Recommendation: adopt for one chart now (`type-donut`), wait on the rest.** Details and the trigger conditions are at the end.

**Status: done.** `type-donut` now ships on TanStack Charts, Recharts and `packages/ui/src/components/chart.tsx` are deleted, and the measured result is in [Outcome](#outcome) at the bottom. `growth-timeline` and `link-graph` are unchanged; their ports live in `packages/viz/spike/` and ship to nobody.

---

## First, a correction to the premise

The brief says `packages/viz` and `packages/ui` depend on Recharts and lists five charts "in play". Both halves of that are true of `package.json` but not of the code:

| Component | What it actually is | Recharts? |
|---|---|---|
| `type-donut.tsx` | `PieChart`/`Pie`/`Cell` via the shadcn `ChartContainer` | **yes — the only one** |
| `growth-timeline.tsx` | hand-written `<svg>`, hand-computed path strings | no |
| `link-graph.tsx` | hand-written `<svg>`, fixed circular layout | no |
| `budget-bar.tsx` | HTML/CSS stacked bar + a `<table>` — **zero `<svg>`** | no |
| `budget-gauge.tsx` | HTML/CSS progress bars — **zero `<svg>`** | no |

`packages/ui/src/components/chart.tsx` is 373 lines of shadcn Recharts wrapper whose only consumer in the entire repo is `type-donut.tsx`.

Also: `apps/web/src/components/sections/viz-surface.tsx`, named in the brief as a consumer, does not exist. The real consumers are `budget-forensics.tsx`, `memory-forensics.tsx`, `timeline-dumbzone.tsx`, `hero.tsx` (web) and `session-detail.tsx`, `vault-section.tsx` (inspector).

This reframes the question. It is not "should we migrate five Recharts charts". It is **"we carry a 98.5 KiB charting library for one 140-pixel donut — is TanStack Charts a better way to pay for that?"**

## What was built

Three ports, on this branch, alongside the originals. No production chart was switched over; nothing in `apps/` imports any of them.

- `packages/viz/src/components/type-donut-tanstack.tsx` — the decisive one, since `type-donut` is the only Recharts chart.
- `packages/viz/src/components/growth-timeline-tanstack.tsx` — the real time series, and the hardest port.
- `packages/viz/src/components/link-graph-tanstack.tsx` — the stress case, using the built-in `forceLayout` instead of the shipping component's fixed ring.
- `packages/viz/spike/ssr-report.tsx` — reproducible SSR harness (`bun run packages/viz/spike/ssr-report.tsx`).

All three typecheck clean under the repo's `tsc --noEmit` with **no casts, no `@ts-expect-error`, no adapter generics** — which the library's own authoring rules ask for and which Recharts' typings do not reliably permit.

## Lines of code

Non-blank, non-comment lines:

| Chart | Current | TanStack | Δ |
|---|---:|---:|---:|
| `growth-timeline` | 333 | 291 | **−13%** |
| `link-graph` | 121 | 150 | +24% |
| `type-donut` | 66 | 83 | +26% |

Read these carefully; none is a like-for-like swap.

- **`growth-timeline`** is the honest comparison and the grammar wins it. The 42 lines saved are entirely the hand-rolled scale functions, the `buildAreas` stacking loop and the manual `d`-string construction. What replaces them is declarative and, unlike the original, is not silently wrong when the container resizes.
- **`link-graph`** grew, but it also gained a real force-directed layout — `forceLayout` with link/manyBody/center/collide forces, deterministic across renders. The shipping version places nodes on a circle, which is cheap and tells you nothing about topology. Getting the same capability by hand would be far more than 29 extra lines.
- **`type-donut`** grew because the original leans on a 373-line shared wrapper that the line count doesn't charge it for. Counting the wrapper, the TanStack version is 83 lines against 439.

## Bundle size

Measured with `bun build --minify`, React/workspace packages external, then `gzip -9`. Per-chart trees, not whole-app builds.

| Bundle | min | min+gzip |
|---|---:|---:|
| Hand-rolled `growth-timeline` + `link-graph` | 8.0 KiB | **3.0 KiB** |
| Recharts (`type-donut` only) | 316.8 KiB | **98.5 KiB** |
| TanStack `type-donut` | 87.6 KiB | 30.4 KiB |
| TanStack `growth-timeline` | 96.3 KiB | 33.0 KiB |
| TanStack `link-graph` | 103.5 KiB | 36.0 KiB |
| TanStack — timeline + graph | 120.6 KiB | 41.4 KiB |
| TanStack — all three | 135.6 KiB | 46.9 KiB |
| **Both libraries** (Recharts + TanStack timeline + graph) | 436.1 KiB | **139.5 KiB** |

The brief's "~27–32 KiB cold-page" figure checks out: a first chart costs ~30 KiB gzip, and each additional chart adds only ~5–8 KiB because the core amortizes (30.4 → 46.9 KiB for two more charts).

The decision-relevant deltas, against today's shipped 101.5 KiB (98.5 Recharts + 3.0 hand-rolled):

| Option | Total | Δ vs today |
|---|---:|---:|
| Port `type-donut` only, keep hand-rolled SVG | **33.4 KiB** | **−68.1 KiB** |
| Port all three to TanStack | 46.9 KiB | −54.6 KiB |
| Ship both libraries (gradual migration) | 139.5 KiB | +38.0 KiB |

**Porting only `type-donut` is the bundle-optimal outcome.** The hand-rolled SVG charts are nearly free; every kilobyte of the win comes from deleting Recharts. Porting them *to* TanStack costs 13.5 KiB rather than saving anything.

## SSR

The inspector serves pages, so this is not academic. From `spike/ssr-report.tsx`, via `renderToString`:

| Component | Server HTML | `<svg>` | Geometry rendered |
|---|---:|---:|---|
| `type-donut` (**Recharts**) | 2,777 B | **0** | **none** |
| `type-donut` (TanStack) | 4,027 B | 1 | 5 arc paths |
| `growth-timeline` (hand-rolled) | 38,669 B | 1 | 14 paths, 76 circles |
| `growth-timeline` (TanStack, default) | 228,236 B | 2 | 13 paths, 1,134 circles |
| `link-graph` (hand-rolled) | 3,651 B | 1 | 9 circles, 7 lines |
| `link-graph` (TanStack) | 10,776 B | 1 | 34 circles, 7 lines |

Two findings, pulling in opposite directions.

**Recharts server-renders nothing.** Not a degraded chart — zero SVG elements. `ChartContainer` wraps everything in `ResponsiveContainer`, which needs a measured DOM box, so the server emits a bare `<div>` and a `<style>` tag. TanStack renders complete, correct SVG on the server with `initialWidth`, then remeasures after hydration. For a served inspector this is a straightforward improvement and is the strongest single argument for the donut port.

**But TanStack's default focus behaviour is expensive.** The 1,134 circles are not marks — the SVG renderer pre-renders one hidden focus ring per interaction point. A 75-turn × 10-category stacked area has 750 interaction points from the area mark alone. Measured directly:

| Config | Server HTML | `<circle>` |
|---|---:|---:|
| defaults | 196,841 B | 975 |
| `focusRing: false` | **41,109 B** | **75** |

**4.8× of the HTML is pre-rendered focus rings.** This is a configuration default, not an inherent cost — but `focusRing: false` removes the visible focus indicator, which the library's own accessibility guide says to do "only when authored focus geometry replaces that". So the byte saving trades directly against keyboard affordance. Anyone shipping a large-N TanStack chart needs to make that call deliberately; the default silently produces ~200 KB of HTML per chart.

## Theming against the shared tokens

TanStack Charts inherits rather than installing a theme: `currentColor` for foreground/muted/grid, `transparent` background, and six `--ts-chart-N` CSS variables you can override at any container boundary. That composes cleanly with `@workspace/ui`'s `--chart-1..5` and the `next-themes` `class` attribute strategy — the donut port passes `var(--chart-N)` straight through as the colour range and needs no light/dark branch at all.

**This surfaced a live bug in the shipping code.** The inspector has a real light/dark/system toggle (`next-themes`, `defaultTheme="system"`), but `growth-timeline.tsx` hard-codes:

```
stroke="rgba(255,255,255,0.07)"   // gridlines
stroke="rgba(255,255,255,0.85)"   // total silhouette
fill="rgba(255,255,255,0.9)"      // per-turn dots
```

In light mode those render white-on-white. The chart is dark-only by accident. The TanStack port uses `currentColor` and is theme-correct for free — not because of any porting effort, but because inheriting is the library's default and hand-rolled SVG has no default at all.

Worth noting separately: `CAT_META` colours in `packages/core` are hard-coded hex (`#6e7681`, `#58a6ff`, …), not tokens. That is a core-side issue and neither library changes it. **This bug is reported, not fixed — fixing it is outside this spike's scope.**

## Accessibility

| | Hand-rolled | Recharts | TanStack |
|---|---|---|---|
| `role="img"` + name | yes (manual) | none server-side | yes (`ariaLabel` **required**) |
| `aria-roledescription` | no | no | yes |
| `<desc>` | no | no | yes, via `ariaDescription` |
| Keyboard focus | no | no | yes, `tabindex=0` + arrow traversal |
| Reduced motion | n/a | partial | `prefers-reduced-motion` respected by default |

TanStack is the only one of the three that makes the accessible name mandatory — `ariaLabel` is a required prop, so an unnamed chart is a type error. Keyboard traversal over data points is native and typed.

Caveats, in fairness:

- The rendered SVG is `role="img"`, so a screen reader gets the name and description, not the data. The library's guidance is explicit that a linked table remains the application's job for exact values. `budget-bar.tsx` already does this correctly for the budget forensic.
- `link-graph`'s current node-level keyboard affordance is per-node `role="button"` + `tabIndex`, which a screen reader can enumerate. The TanStack port replaces this with one focusable surface and typed `onSelect`. That is better keyboard ergonomics and arguably worse enumerability. **Not a straight upgrade** — it would need a linked list of nodes beside the graph to be equivalent.
- The `onSelect` datum is a union across every mark in the definition, so a node click must be narrowed (`"id" in datum`). Honest typing, mildly awkward.

## Coexistence

**Yes, cleanly, with no all-or-nothing constraint.** Verified by rendering Recharts and two TanStack charts in one React tree and one document:

- Three chart hosts rendered together, no errors.
- **Zero duplicate DOM ids.** TanStack scopes SVG resource ids through React's `useId` (`ts-chart-_R_q_-ts-chart-clip-…`), so clip paths and gradients cannot collide between instances or with Recharts.
- No global CSS, no shared registry, no conflicting peer deps. Both sit in `packages/viz`'s `dependencies` simultaneously and tree-shake independently.

One gap worth knowing: TanStack declares **gradients** as chart resources but has no **pattern** equivalent. `growth-timeline`'s hatch overlay on inferred slices therefore needs an application-owned `<defs>` in a sibling SVG, referenced cross-document as `url(#id)` — and that id must be `useId`-scoped by hand, or two timelines on one page collide. It works, but it is a genuine expressiveness gap, and the visual result was **not verified in a browser** (this environment cannot launch one).

## The pre-alpha risk

This is the part that decides the answer, so it was measured rather than assumed.

**The library is ten days old.** First publish `0.0.0` on 2026-07-29; `0.7.2` on 2026-08-07. Nineteen releases in ten days. The three most recent — `0.7.0`, `0.7.1`, `0.7.2` — all shipped *the day before this spike ran*.

Churn between releases is real:

- `0.6.5` → `0.7.2`, a three-day window: subpath exports went 58 → 91, `dist` files 154 → 300, and **31 of 77 shared `.d.ts` files changed** — including `dot`, `line`, `stack` and `types`, the exact APIs these ports use.

But the *character* of that churn is better than the volume suggests:

- **No subpath export has ever been removed** across `0.3.0` → `0.7.2`. Every diff is additive.
- The type changes sampled are additive too: `dot` gained an optional `layout`; `stack` gained `'inside-out'` and `anchor`. Existing call sites keep their meaning.
- **Empirical check:** all three ports were recompiled unchanged against `0.7.0` and `0.7.1`. Both typecheck clean. The API these charts touch did not break across the last two releases.

Against that, two things temper the reassurance:

1. Ten days is not enough history to extrapolate from. "No removals yet" over a week-and-a-half is weak evidence about the next six months, and the docs explicitly reserve the right to break between releases.
2. **`network/force` — the entire reason the `link-graph` port is interesting — was added in `0.7.0`, one day ago.** It has no track record whatsoever. The parts of the library the ambitious ports depend on are the newest and least settled parts.

The concrete exposure differs sharply by chart, which is what makes a partial answer possible:

| Chart | API surface used | Exposure |
|---|---|---|
| `type-donut` | `pie`, `polar`, `radialArc`, `defineChart` — ~4 calls, 83 lines | **low** |
| `growth-timeline` | 8 mark types, stack layout, 2 scales, annotations | high |
| `link-graph` | `forceLayout` (1 day old), `link`, `dot`, ordinal colour | **highest** |

A breaking change to the donut is an afternoon. A breaking change to the timeline is a re-port.

Peektrace ships a desktop app and a served inspector, so a dependency that breaks on upgrade is a release-blocking cost, not an inconvenience. But that argument cuts against *breadth of exposure*, not against adoption as such — 83 lines behind one component boundary is a genuinely different risk from three charts and 500 lines.

## Recommendation

**Adopt for `type-donut` now. Keep the hand-rolled SVG charts as they are. Revisit the rest at 1.0.**

Porting `type-donut` and deleting Recharts + `packages/ui/src/components/chart.tsx`:

- removes **68 KiB gzip** — the single largest bundle win available in the front end;
- gives the served inspector a donut that actually server-renders, instead of a blank div;
- deletes 373 lines of shadcn wrapper and its `[&_.recharts-*]` selector wall;
- exposes ~83 lines behind one component boundary, so a breaking upgrade is bounded;
- costs nothing elsewhere, because coexistence is clean and no other file imports Recharts.

Not porting `growth-timeline` and `link-graph`:

- they cost **3.0 KiB gzip today**. Porting them *adds* 13.5 KiB.
- they work, and they are the two highest-churn API surfaces in the library.
- `link-graph`'s port depends on a transform that is one day old.
- the timeline's hatch pattern needs a workaround the library does not support natively.

The migration size if the answer were yes to everything: three charts, ~520 lines of new component code, plus deleting `chart.tsx` and one dependency. Small — a day or two of work. Size is not the constraint here; churn exposure is.

### What would change the answer

Port the remaining two charts when **any two** of these hold:

1. **`@tanstack/charts` reaches 1.0**, or the docs drop the "API subject to change between releases" clause.
2. **Ninety days pass with no removed subpath export and no breaking type change** to `areaY`, `dot`, `link`, `stack` or `forceLayout`. The additive-only pattern is real but only ten days deep; three months would make it a trend.
3. **`forceLayout` acquires a track record** — it is currently one release old, and it is the whole value proposition for `link-graph`.
4. **A pattern/texture resource lands** alongside `gradients`, removing the cross-SVG `url(#…)` workaround the timeline's hatched slices need.

Two changes would move the answer *sooner*:

- If the shipping `growth-timeline` needs to work in light mode — it currently does not, per the hard-coded white strokes above — then a port becomes a fix rather than a rewrite, and the calculus changes. Fixing the three hard-coded colours in place is still cheaper.
- If a second genuine chart is ever added to the product, TanStack's marginal cost (~5–8 KiB) beats both Recharts and hand-rolled SVG, and adopting it broadly becomes the cheaper default.

Two things would move it *later*: any removed export before 1.0, or a breaking change to the polar entry.

## Outcome

The donut port landed. What follows is measured on the real app, not on per-chart
entry points, and corrects two things the spike got wrong.

### Bundle

Whole `apps/inspector` production build, every asset in `dist/assets`, `gzip -9`:

| Chunk | Before | After | Δ |
|---|---:|---:|---:|
| `vendor` (Recharts lived here) | 163.78 KiB | 104.23 KiB | **−59.6 KiB** |
| `index` (app) | 37.30 KiB | 36.03 KiB | −1.3 KiB |
| `index.css` | 25.19 KiB | 24.60 KiB | −0.6 KiB |
| `vendor-radix` | 19.87 KiB | 19.71 KiB | −0.2 KiB |
| **Total dist** | **388.3 KiB** | **328.9 KiB** | **−59.3 KiB (−15.2%)** |

Modules transformed dropped 1742 → 1305. The >500 KB chunk warning no longer
fires. The CSS shrank because `chart.tsx`'s `[&_.recharts-*]` selector wall went
with it.

The spike predicted −68.1 KiB from per-chart trees. The real figure is −59.3 KiB,
because TanStack shares `d3-*` and `es-toolkit` with code the inspector already
loads, so its 30.4 KiB does not land whole.

### The donut itself

Before, the three slices were one flat ring: `--chart-1..5` are five lightness
steps of a single cyan hue, and Recharts drew them edge to edge. A `gapAngle`
seam now separates them, which is the only deliberate visual change. The ring
also fills its 140px box, because `ResponsiveContainer` was insetting it.

Verified in a browser in both themes, which the spike could not do — this closes
its "Unverified" list for the donut. Hover tooltip, keyboard focus and arrow
traversal all work; the emitted SVG carries `role="img"`, `aria-label` and
`aria-roledescription="chart"`.

### Correction: SSR was not the reason

The spike argued the served inspector gains a donut that server-renders. It does
not. `peektrace serve` hosts a static Vite `dist/`; the inspector is a SPA with
no server render at any point, and the donut is inspector-only — `apps/web` never
imported it. The SSR table stands as a property of the libraries and is why the
harness is kept, but it did not apply to this migration. The real wins were
bundle size and the flat-ring rendering.

## Reproducing

```sh
bun run packages/viz/spike/ssr-report.tsx   # SSR comparison table
cd packages/viz && bun run typecheck        # shipping donut + both ports, no casts
bun run --filter=inspector build            # bundle figures in the Outcome table
```

Per-chart bundle figures in the earlier tables: `bun build packages/viz/<entry>.tsx --target=browser --minify --external react --external react-dom --external '@workspace/*'`, then `gzip -9`.

## Still unverified

The donut was rendered and checked in both themes before it shipped, so the
original caveats no longer apply to it. They still apply to the two unported
candidates in `packages/viz/spike/`:

- Neither was rendered visually. Geometry is inferred from SSR SVG output, not from looking at it.
- The timeline's hatch pattern emits its cross-SVG `url(#…)` reference correctly but was never painted.
- No screen-reader testing against a live DOM for either.
