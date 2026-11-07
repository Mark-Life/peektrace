const STATS_POINTS = [
  {
    title: "Retries",
    body: "A failed call followed by a similar call that worked. Each cause shows the seconds and context tokens the retries re-read, and a one-line fix.",
  },
  {
    title: "Failed tool calls",
    body: "Errors grouped by shape across every session, with command text redacted. Fix the cheapest cause first.",
  },
  {
    title: "Wait time",
    body: "Real work is separated from parked polling, so the waiting number is not padded.",
  },
] as const;

/**
 * Text-only Stats section: what `peektrace stats` ranks across all sessions.
 */
export const Stats = () => (
  <section className="py-20" id="stats">
    <div className="mx-auto max-w-6xl px-6">
      <span className="font-mono text-primary text-xs uppercase tracking-[0.2em]">
        STATS
      </span>
      <h2 className="mt-4 text-balance font-heading text-3xl tracking-tight md:text-4xl">
        See what fails and what costs time, across every session.
      </h2>
      <p className="mt-4 max-w-2xl text-lg text-muted-foreground">
        Open Stats in the app, or run{" "}
        <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-foreground">
          peektrace stats
        </code>
        . Findings come sorted by cheapest fix first.
      </p>
      <dl className="mt-10 grid gap-6 md:grid-cols-3">
        {STATS_POINTS.map((point) => (
          <div
            className="rounded-xl border border-border p-5"
            key={point.title}
          >
            <dt className="font-heading text-lg">{point.title}</dt>
            <dd className="mt-2 text-muted-foreground text-sm">{point.body}</dd>
          </div>
        ))}
      </dl>
    </div>
  </section>
);
