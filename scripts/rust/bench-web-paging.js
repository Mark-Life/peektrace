// Page probe for the web-paging suite of bench.sh, run as an agent-browser
// init script. bench.sh writes `var BENCH_SCROLL = 0|1;` before this file.
// Marks are `performance.now()` (ms since navigation start), checked once per
// animation frame. With BENCH_SCROLL, 1 s after the first history row is drawn
// the probe moves the history's scroll pane one viewport per frame to its end.
(() => {
  const b = {};
  window.__bench = b;
  const scroll = globalThis.BENCH_SCROLL === 1;
  const now = () => performance.now();
  const $ = (s) => document.querySelector(s);
  const want = location.hash.includes("id=") ? "row" : "list";
  // Paged scrolling can pass the default 250 resource entries.
  performance.setResourceTimingBufferSize(100_000);
  new PerformanceObserver((l) => {
    for (const e of l.getEntries()) {
      if (e.name === "first-contentful-paint") {
        b.fcp ??= e.startTime;
      }
    }
  }).observe({ type: "paint", buffered: true });

  // Bytes received so far (Resource Timing transferSize): page, assets and API; API alone.
  const bytes = () => {
    let all = performance.getEntriesByType("navigation")[0]?.transferSize ?? 0;
    let api = 0;
    let calls = 0;
    for (const e of performance.getEntriesByType("resource")) {
      all += e.transferSize;
      if (new URL(e.name).pathname.startsWith("/api/")) {
        api += e.transferSize;
        calls++;
      }
    }
    return { all, api, calls };
  };
  const mark = (k, seen) => {
    if (b[k] != null || !seen) {
      return false;
    }
    b[k] = now();
    return true;
  };
  const finish = () => {
    b.done = true;
  };

  // The pane the history scrolls in: its nearest scrolling ancestor.
  const scrollRoot = () => {
    for (
      let el = $("[data-testid=session-history]");
      el;
      el = el.parentElement
    ) {
      const o = getComputedStyle(el).overflowY;
      if (
        (o === "auto" || o === "scroll") &&
        el.scrollHeight > el.clientHeight + 1
      ) {
        return el;
      }
    }
    return document.scrollingElement;
  };
  const pct = (xs, p) => xs[Math.min(xs.length - 1, Math.floor(xs.length * p))];

  // End = at the bottom, no placeholder rows, scroll height unchanged for 500 ms.
  const scrollToEnd = () => {
    const root = scrollRoot();
    const hist = $("[data-testid=session-history]");
    root.scrollTop +=
      hist.getBoundingClientRect().top - root.getBoundingClientRect().top;
    const t0 = now();
    const start = bytes();
    const frames = [];
    let prev = t0;
    let quietFrom = null;
    let height = -1;
    const step = () => {
      const t = now();
      frames.push([t, t - prev]);
      prev = t;
      const max = root.scrollHeight - root.clientHeight;
      if (root.scrollTop < max - 1) {
        root.scrollTop = Math.min(max, root.scrollTop + root.clientHeight);
        quietFrom = null;
      } else {
        b.scrollBottom ??= t - t0;
        const quiet =
          !$("[data-testid=history-pending]") && root.scrollHeight === height;
        quietFrom = quiet ? (quietFrom ?? t) : null;
        root.scrollTop = max;
      }
      height = root.scrollHeight;
      const settled = quietFrom != null && t - quietFrom >= 500;
      if (settled || t - t0 > 90_000) {
        const end = settled ? quietFrom : t;
        const gaps = frames
          .filter(([at]) => at <= end)
          .map(([, g]) => g)
          .sort((x, y) => x - y);
        const got = bytes();
        b.scrollEnd = end - t0;
        b.scrollTimedOut = !settled;
        b.scrollHeight = root.scrollHeight;
        b.frames = gaps.length;
        b.frameP50 = pct(gaps, 0.5);
        b.frameP95 = pct(gaps, 0.95);
        b.frameMax = gaps.at(-1);
        b.over16 = gaps.filter((g) => g > 1000 / 60).length;
        b.over50 = gaps.filter((g) => g > 50).length;
        b.scrollApi = got.api - start.api;
        b.scrollCalls = got.calls - start.calls;
        finish();
        return;
      }
      requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  };

  let last = 0;
  b.maxGap = 0;
  const tick = () => {
    const t = now();
    if (last) {
      b.maxGap = Math.max(b.maxGap, t - last);
    }
    last = t;
    mark("list", $("[data-session-id]"));
    mark("summary", $("[data-testid=verdict-header]"));
    mark("chart", $("[data-testid=timeline-svg]"));
    mark("row", $("[data-testid=history-event]"));
    for (const e of performance.getEntriesByType("paint")) {
      if (e.name === "first-contentful-paint") {
        b.fcp ??= e.startTime;
      }
    }
    if (b[want] != null && b.bytes == null) {
      b.bytes = bytes();
    }
    if (scroll && want === "row" && b.row != null && t > b.row + 1000) {
      scrollToEnd();
      return;
    }
    // FCP can land after the target frame: wait up to 2 s more for it.
    if (!scroll && b[want] != null && (b.fcp != null || t > b[want] + 2000)) {
      finish();
      return;
    }
    if (t > 60_000) {
      finish();
      return;
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
})();
