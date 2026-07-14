/**
 * The startup screen, rendered via `webview.loadHTML` while the sidecar boots.
 *
 * It is a static, non-interactive page, so a plain HTML string is enough — no
 * bundled view or RPC. The crash screen, by contrast, needs a "Restart" button
 * that calls back into Bun, so it lives in `src/views/crash/` as a real view.
 */
export const startupHtml = (): string => `<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>Peektrace</title>
    <style>
      :root {
        color-scheme: light dark;
        --background: #0a0a0a;
        --foreground: #fafafa;
        --muted: #a1a1aa;
        --line: #27272a;
        --pulse: #fafafa;
      }
      @media (prefers-color-scheme: light) {
        :root {
          --background: #f7f7f4;
          --foreground: #18181b;
          --muted: #60646c;
          --line: #d8d8d0;
          --pulse: #18181b;
        }
      }
      body {
        margin: 0;
        min-height: 100vh;
        display: grid;
        place-items: center;
        background: var(--background);
        color: var(--foreground);
        font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      }
      main {
        width: min(22rem, calc(100vw - 4rem));
        display: grid;
        gap: 1rem;
        justify-items: center;
        text-align: center;
      }
      .mark {
        width: 2.25rem;
        height: 2.25rem;
        border: 1px solid var(--line);
        border-radius: 8px;
        display: grid;
        place-items: center;
        font-size: 0.82rem;
        font-weight: 650;
      }
      h1 {
        margin: 0;
        font-size: 1.05rem;
        font-weight: 600;
      }
      p {
        margin: -0.35rem 0 0;
        color: var(--muted);
        font-size: 0.82rem;
        line-height: 1.5;
      }
      .activity {
        width: 8rem;
        height: 2px;
        overflow: hidden;
        border-radius: 999px;
        background: var(--line);
      }
      .activity::after {
        content: "";
        display: block;
        width: 45%;
        height: 100%;
        border-radius: inherit;
        background: var(--pulse);
        animation: slide 1.15s ease-in-out infinite;
      }
      @keyframes slide {
        0% { transform: translateX(-110%); opacity: 0.35; }
        50% { opacity: 0.9; }
        100% { transform: translateX(245%); opacity: 0.35; }
      }
      @media (prefers-reduced-motion: reduce) {
        .activity::after { animation: none; transform: none; width: 100%; opacity: 0.55; }
      }
      .titlebar {
        position: fixed;
        top: 0;
        left: 0;
        right: 0;
        height: 32px;
      }
    </style>
  </head>
  <body>
    <div class="titlebar electrobun-webkit-app-region-drag"></div>
    <main>
      <div class="mark">Pe</div>
      <h1>Starting Peektrace&hellip;</h1>
      <p>Preparing your local workspace.</p>
      <div class="activity" aria-hidden="true"></div>
    </main>
  </body>
</html>`;
