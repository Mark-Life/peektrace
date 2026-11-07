import { CommandBlock } from "@/components/command-block";
import { ThemeToggle } from "@/components/theme-toggle";

const INSTALL_COMMAND =
  "curl -fsSL https://raw.githubusercontent.com/Mark-Life/peektrace/main/scripts/install.sh | sh";

interface FooterLink {
  href: string;
  label: string;
}

const LINKS: readonly FooterLink[] = [
  {
    label: "Docs (README)",
    href: "https://github.com/Mark-Life/peektrace#readme",
  },
  { label: "GitHub", href: "https://github.com/Mark-Life/peektrace" },
  { label: "Capability matrix", href: "#agents-capabilities" },
  {
    label: "Report a bug",
    href: "https://github.com/Mark-Life/peektrace/issues",
  },
];

const BOTTOM_NOTES: readonly string[] = [
  "loopback only",
  "not a cloud service",
  "no account, no cloud",
  "your transcripts stay on your machine",
];

/**
 * Site footer: brand block, compact copyable install line, links column, and a
 * mono bottom bar carrying the privacy notes plus the theme toggle. Renders a
 * real <footer> landmark and owns the page's only ThemeToggle.
 */
export const SiteFooter = () => (
  <footer className="w-full border-border border-t py-12">
    <div className="mx-auto max-w-6xl px-6">
      <div className="grid gap-8 md:grid-cols-3">
        <div className="flex flex-col gap-3">
          <div className="flex items-center gap-3">
            <span
              aria-hidden="true"
              className="grid size-8 place-items-center rounded-lg bg-primary font-heading font-semibold text-primary-foreground"
            >
              P
            </span>
            <span className="font-heading font-semibold text-foreground text-lg">
              Peektrace
            </span>
          </div>
          <p className="text-muted-foreground text-sm">
            Agent session inspector
          </p>
        </div>

        <div className="flex flex-col justify-start">
          <CommandBlock command={INSTALL_COMMAND} />
        </div>

        <nav aria-label="Footer" className="flex flex-col gap-3">
          <h2 className="font-heading font-semibold text-foreground text-sm">
            Links
          </h2>
          <ul className="flex flex-col gap-2">
            {LINKS.map(({ label, href }) => (
              <li key={label}>
                <a
                  className="rounded-sm text-muted-foreground text-sm transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
                  href={href}
                >
                  {label}
                </a>
              </li>
            ))}
          </ul>
        </nav>
      </div>

      <div className="mt-12 flex flex-col items-start gap-4 border-border border-t pt-6 sm:flex-row sm:items-center sm:justify-between">
        <ul className="flex flex-wrap items-center gap-x-2 gap-y-1 font-mono text-muted-foreground text-xs">
          {BOTTOM_NOTES.map((note) => (
            <li className="flex items-center gap-2" key={note}>
              <span>{note}</span>
              <span aria-hidden="true">&middot;</span>
            </li>
          ))}
          <li>&copy; 2026 Peektrace</li>
        </ul>
        <ThemeToggle />
      </div>
    </div>
  </footer>
);
