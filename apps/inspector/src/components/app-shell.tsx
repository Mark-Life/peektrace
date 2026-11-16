/** App shell: collapsible left nav (Sessions / Memory / Capabilities) + content pane.
 *
 * Dark-first, dense, forensic. Nav items drive the hash router; the active
 * section is highlighted. The sidebar collapses to an icon rail (⌘B, the rail
 * edge, or the header trigger) so wide session views get the pixels back.
 */
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarRail,
  SidebarTrigger,
  useSidebar,
} from "@workspace/ui/components/sidebar";
import { cn } from "@workspace/ui/lib/utils";
import {
  ActivityIcon,
  DatabaseIcon,
  LayoutGridIcon,
  MessagesSquareIcon,
  SettingsIcon,
} from "lucide-react";
import { useState } from "react";
import type { ComponentType, ReactNode } from "react";

import { useDocumentTitle } from "../lib/document-title";
import { MAIN_CONTENT_ID } from "../lib/landmarks";
import { navigate, useRoute } from "../lib/routes";
import type { RouteId } from "../lib/routes";
import { ScrollRootContext } from "../lib/scroll-root";
import { ThemeToggle, useThemeShortcut } from "../lib/theme";
import { ReadOnlyBadge } from "./read-only";
import { ShortcutsButton, ShortcutsDialog } from "./shortcuts-dialog";
import { ShellSkipLinks } from "./skip-links";
import { WatchStatus } from "./watch-status";

/** Nav entry: section id, label, and its icon. */
interface NavItem {
  readonly hint: string;
  readonly icon: ComponentType<{ className?: string }>;
  readonly id: RouteId;
  readonly label: string;
}

/** The sections, in display order. */
const NAV_ITEMS: readonly NavItem[] = [
  {
    hint: "Context debug",
    icon: MessagesSquareIcon,
    id: "sessions",
    label: "Sessions",
  },
  {
    hint: "What fails, what costs time",
    icon: ActivityIcon,
    id: "stats",
    label: "Stats",
  },
  { hint: "All projects", icon: DatabaseIcon, id: "memory", label: "Memory" },
  {
    hint: "Agent roots",
    icon: SettingsIcon,
    id: "settings",
    label: "Settings",
  },
];

/** Pages reached by a link, not the rail; they still title the header. */
const PAGE_ITEMS: readonly NavItem[] = [
  {
    hint: "Support matrix",
    icon: LayoutGridIcon,
    id: "capabilities",
    label: "Capabilities",
  },
];

const SIDEBAR_COOKIE = /(?:^|;\s*)sidebar_state=(?<state>true|false)/u;

/** Content header: section title, the sidebar toggle, the sync dot and the
 *  read-only badge. */
const ShellHeader = ({ active }: { readonly active?: NavItem }) => (
  <header className="border-border flex h-11 shrink-0 items-center gap-2 border-b px-3">
    <SidebarTrigger className="-ml-1" />
    <span className="text-sm font-medium">{active?.label}</span>
    <span className="text-muted-foreground text-xs">{active?.hint}</span>
    <WatchStatus />
    <ReadOnlyBadge />
  </header>
);

/** Persisted open/closed state, written by `SidebarProvider` as a cookie. */
const readSidebarCookie = () =>
  SIDEBAR_COOKIE.exec(document.cookie)?.groups?.state !== "false";

/** Footer row: theme toggle and the shortcuts sheet, left-aligned when
 *  expanded, stacked and centred on the icon rail. */
const ShellFooter = ({ onHelp }: { readonly onHelp: () => void }) => {
  const collapsed = useSidebar().state === "collapsed";

  return (
    <div
      className={cn(
        "flex items-center gap-2",
        collapsed ? "flex-col justify-center" : "justify-start"
      )}
    >
      <ThemeToggle iconOnly={collapsed} />
      <ShortcutsButton iconOnly={collapsed} onOpen={onHelp} />
    </div>
  );
};

/** The shell: collapsible sidebar + a scrollable content region. */
export const AppShell = ({ children }: { readonly children: ReactNode }) => {
  const route = useRoute();

  const active = [...NAV_ITEMS, ...PAGE_ITEMS].find(
    (item) => item.id === route
  );

  const [scrollRoot, setScrollRoot] = useState<HTMLElement | null>(null);
  const [helpOpen, setHelpOpen] = useState(false);
  useThemeShortcut();
  useDocumentTitle(active?.label ?? null);

  return (
    <SidebarProvider
      className="h-dvh min-h-0"
      defaultOpen={readSidebarCookie()}
    >
      <ShellSkipLinks />
      <Sidebar collapsible="icon">
        <SidebarHeader>
          <div className="flex h-8 items-center gap-2 overflow-hidden group-data-[collapsible=icon]:justify-center">
            <div className="bg-primary/15 text-primary flex size-8 shrink-0 items-center justify-center rounded-md text-xs font-bold">
              P
            </div>
            <div className="flex min-w-0 flex-col leading-tight group-data-[collapsible=icon]:hidden">
              <span className="truncate text-sm font-semibold">Peektrace</span>
              <span className="text-muted-foreground truncate text-xs">
                Agent session inspector
              </span>
            </div>
          </div>
        </SidebarHeader>
        <SidebarContent>
          <SidebarGroup>
            <SidebarGroupContent>
              <SidebarMenu className="group-data-[collapsible=icon]:items-center">
                {NAV_ITEMS.map((item) => (
                  <SidebarMenuItem key={item.id}>
                    <SidebarMenuButton
                      aria-label={item.label}
                      className="group-data-[collapsible=icon]:justify-center"
                      isActive={route === item.id}
                      onClick={() => {
                        navigate(item.id);
                      }}
                      tooltip={`${item.label} · ${item.hint}`}
                    >
                      <item.icon className="size-4 shrink-0" />
                      <span className="truncate text-sm font-medium group-data-[collapsible=icon]:hidden">
                        {item.label}
                      </span>
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                ))}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        </SidebarContent>
        <SidebarFooter>
          <ShellFooter
            onHelp={() => {
              setHelpOpen(true);
            }}
          />
        </SidebarFooter>
        <SidebarRail />
      </Sidebar>
      <SidebarInset className="min-w-0 overflow-hidden">
        <ShellHeader active={active} />
        <div
          className="min-h-0 flex-1 scrollbar-gutter-stable overflow-y-auto outline-none"
          id={MAIN_CONTENT_ID}
          ref={setScrollRoot}
          tabIndex={-1}
        >
          <div
            className={cn(
              "mx-auto px-3 py-4 md:px-6 md:py-6",
              route === "sessions" ? "max-w-[1800px]" : "max-w-6xl"
            )}
          >
            <ScrollRootContext value={scrollRoot}>{children}</ScrollRootContext>
          </div>
        </div>
      </SidebarInset>
      <ShortcutsDialog onOpenChange={setHelpOpen} open={helpOpen} />
    </SidebarProvider>
  );
};
