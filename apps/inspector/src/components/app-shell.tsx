/** App shell: fixed left nav (Memory / Sessions / Capabilities) + content pane.
 *
 * Dark-first, dense, forensic. Nav items drive the hash router; the active
 * section is highlighted. Kept deliberately light — no sidebar provider, just a
 * flex column — so the shell stays legible and fast.
 */
import { cn } from "@workspace/ui/lib/utils";
import { DatabaseIcon, LayoutGridIcon, MessagesSquareIcon } from "lucide-react";
import type { ComponentType, ReactNode } from "react";
import { navigate, type RouteId, useRoute } from "../lib/routes";
import { ThemeToggle } from "../lib/theme";

/** Nav entry: section id, label, and its icon. */
interface NavItem {
  readonly hint: string;
  readonly icon: ComponentType<{ className?: string }>;
  readonly id: RouteId;
  readonly label: string;
}

/** The three sections, in display order. */
const NAV_ITEMS: readonly NavItem[] = [
  {
    id: "sessions",
    label: "Sessions",
    icon: MessagesSquareIcon,
    hint: "Context debug",
  },
  { id: "memory", label: "Memory", icon: DatabaseIcon, hint: "All projects" },
  {
    id: "capabilities",
    label: "Capabilities",
    icon: LayoutGridIcon,
    hint: "Support matrix",
  },
];

/** A single nav button. */
const NavButton = ({
  item,
  active,
}: {
  readonly item: NavItem;
  readonly active: boolean;
}) => {
  const Icon = item.icon;
  return (
    <button
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex w-full items-center gap-3 rounded-md px-3 py-2 text-left text-sm transition-colors",
        active
          ? "bg-primary/10 text-foreground"
          : "text-muted-foreground hover:bg-muted hover:text-foreground"
      )}
      onClick={() => navigate(item.id)}
      type="button"
    >
      <Icon className="size-4 shrink-0" />
      <span className="flex flex-col">
        <span className="font-medium">{item.label}</span>
        <span className="text-muted-foreground text-xs">{item.hint}</span>
      </span>
    </button>
  );
};

/** The shell: sidebar + a scrollable content region. */
export const AppShell = ({ children }: { readonly children: ReactNode }) => {
  const route = useRoute();
  return (
    <div className="flex h-dvh w-full bg-background text-foreground">
      <aside className="flex w-60 shrink-0 flex-col border-border border-r">
        <div className="flex items-center gap-2 px-4 py-4">
          <div className="flex size-7 items-center justify-center rounded-md bg-primary/15 font-bold text-primary text-xs">
            P
          </div>
          <div className="flex flex-col leading-tight">
            <span className="font-semibold text-sm">Peephole</span>
            <span className="text-muted-foreground text-xs">
              Claude inspector
            </span>
          </div>
        </div>
        <nav className="flex flex-col gap-1 px-2 py-2">
          {NAV_ITEMS.map((item) => (
            <NavButton active={route === item.id} item={item} key={item.id} />
          ))}
        </nav>
        <div className="mt-auto flex items-center justify-between border-border border-t px-3 py-2">
          <span className="text-muted-foreground text-xs">loopback only</span>
          <ThemeToggle />
        </div>
      </aside>
      <main className="flex-1 overflow-y-auto">
        <div
          className={cn(
            "mx-auto px-6 py-6",
            route === "sessions" ? "max-w-[1800px]" : "max-w-6xl"
          )}
        >
          {children}
        </div>
      </main>
    </div>
  );
};
