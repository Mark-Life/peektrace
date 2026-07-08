/**
 * Inline screens rendered as `data:` URLs in the main BrowserWindow, so the
 * preload bridge stays available and their buttons can drive the same
 * `window.peephole` IPC the app uses. `startupWindowHtml` shows while the
 * sidecar boots; `sidecarCrashHtml` replaces the dead web UI when the sidecar
 * exits under a live window, offering a one-click restart.
 */

export const startupWindowHtml = (): string => `<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>Peephole</title>
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
    </style>
  </head>
  <body>
    <main>
      <div class="mark">Pe</div>
      <h1>Starting Peephole&hellip;</h1>
      <p>Preparing your local workspace.</p>
      <div class="activity" aria-hidden="true"></div>
    </main>
  </body>
</html>`;

export const sidecarCrashHtml = (): string => `<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>Peephole</title>
    <style>
      :root { color-scheme: dark; }
      body {
        margin: 0;
        min-height: 100vh;
        display: grid;
        place-items: center;
        background: #0a0a0a;
        color: #fafafa;
        font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      }
      .card { max-width: 26rem; padding: 2rem; text-align: center; }
      .icon { font-size: 2rem; margin-bottom: 0.75rem; }
      h1 { font-size: 1.15rem; font-weight: 600; margin: 0 0 0.5rem; }
      p { font-size: 0.875rem; color: #a1a1aa; line-height: 1.5; margin: 0 0 1.5rem; }
      .row { display: flex; gap: 0.6rem; justify-content: center; }
      button {
        padding: 0.55rem 1.1rem;
        border-radius: 6px;
        border: 1px solid transparent;
        background: #fafafa;
        color: #0a0a0a;
        font: inherit;
        font-size: 0.875rem;
        cursor: pointer;
        white-space: nowrap;
      }
      #status { margin-top: 1.25rem; min-height: 1.2em; font-size: 0.8rem; color: #a1a1aa; }
    </style>
  </head>
  <body>
    <main class="card">
      <div class="icon">&#9888;&#65039;</div>
      <h1>The local Peephole server stopped unexpectedly</h1>
      <p>Your data is safe. Restart the server to keep working.</p>
      <div class="row">
        <button id="restart">Restart server</button>
      </div>
      <p id="status"></p>
    </main>
    <script>
      const status = document.getElementById("status");
      document.getElementById("restart").addEventListener("click", async () => {
        status.textContent = "Restarting\\u2026";
        try {
          // Main restarts the sidecar and reloads this window on success.
          await window.peephole.restartServer();
        } catch {
          status.textContent = "Restart failed \\u2014 try quitting and reopening Peephole.";
        }
      });
    </script>
  </body>
</html>`;
