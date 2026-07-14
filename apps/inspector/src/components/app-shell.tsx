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
import { DatabaseIcon, LayoutGridIcon, MessagesSquareIcon } from "lucide-react";
import type { ComponentType, CSSProperties, ReactNode } from "react";
import { hasInsetTitlebar } from "../lib/host";
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

const SIDEBAR_COOKIE = /(?:^|;\s*)sidebar_state=(true|false)/;

/** Space the macOS traffic lights need, kept clear of app chrome. */
const TITLEBAR_HEIGHT = "h-8";

/**
 * The lights end at x≈58, past the stock 3rem icon rail, so the desktop rail is
 * widened to keep them inside the sidebar in either state — the content pane
 * never has to make room for window chrome.
 */
const INSET_RAIL_WIDTH = "5rem";

/** Reserves the titlebar row above the sidebar's own header. */
const TitlebarSpacer = () =>
  hasInsetTitlebar() ? (
    <div aria-hidden="true" className={cn(TITLEBAR_HEIGHT, "shrink-0")} />
  ) : null;

/** Drags the window from the titlebar row (Electrobun reads this class). */
const TitlebarDragRegion = () => {
  const collapsed = useSidebar().state === "collapsed";
  if (!hasInsetTitlebar()) {
    return null;
  }
  return (
    <div
      className={cn(
        "electrobun-webkit-app-region-drag fixed top-0 left-0 z-30",
        TITLEBAR_HEIGHT,
        collapsed ? "w-(--sidebar-width-icon)" : "w-(--sidebar-width)"
      )}
    />
  );
};

/** Content header: section title, plus the sidebar toggle. */
const ShellHeader = ({ active }: { readonly active?: NavItem }) => (
  <header className="flex h-11 shrink-0 items-center gap-2 border-border border-b px-3">
    <SidebarTrigger className="-ml-1" />
    <span className="font-medium text-sm">{active?.label}</span>
    <span className="text-muted-foreground text-xs">{active?.hint}</span>
  </header>
);

/** Persisted open/closed state, written by `SidebarProvider` as a cookie. */
const readSidebarCookie = () =>
  document.cookie.match(SIDEBAR_COOKIE)?.[1] !== "false";

/** Footer row: reach note + theme toggle, both shrinking to fit the icon rail. */
const ShellFooter = () => {
  const collapsed = useSidebar().state === "collapsed";
  return (
    <div
      className={cn(
        "flex items-center gap-2",
        collapsed ? "justify-center" : "justify-between"
      )}
    >
      {collapsed ? null : (
        <span className="truncate text-muted-foreground text-xs">
          loopback only
        </span>
      )}
      <ThemeToggle iconOnly={collapsed} />
    </div>
  );
};

/** The shell: collapsible sidebar + a scrollable content region. */
export const AppShell = ({ children }: { readonly children: ReactNode }) => {
  const route = useRoute();
  const active = NAV_ITEMS.find((item) => item.id === route);
  return (
    <SidebarProvider
      className="h-dvh min-h-0"
      defaultOpen={readSidebarCookie()}
      style={
        hasInsetTitlebar()
          ? ({ "--sidebar-width-icon": INSET_RAIL_WIDTH } as CSSProperties)
          : undefined
      }
    >
      <TitlebarDragRegion />
      <Sidebar collapsible="icon">
        <TitlebarSpacer />
        <SidebarHeader>
          <div className="flex h-8 items-center gap-2 overflow-hidden group-data-[collapsible=icon]:justify-center">
            <div className="flex size-8 shrink-0 items-center justify-center rounded-md bg-primary/15 font-bold text-primary text-xs">
              P
            </div>
            <div className="flex min-w-0 flex-col leading-tight group-data-[collapsible=icon]:hidden">
              <span className="truncate font-semibold text-sm">Peektrace</span>
              <span className="truncate text-muted-foreground text-xs">
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
                      onClick={() => navigate(item.id)}
                      tooltip={`${item.label} · ${item.hint}`}
                    >
                      <item.icon className="size-4 shrink-0" />
                      <span className="truncate font-medium text-sm group-data-[collapsible=icon]:hidden">
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
          <ShellFooter />
        </SidebarFooter>
        <SidebarRail />
      </Sidebar>
      <SidebarInset className="min-w-0 overflow-hidden">
        <ShellHeader active={active} />
        <div className="scrollbar-gutter-stable min-h-0 flex-1 overflow-y-auto">
          <div
            className={cn(
              "mx-auto px-6 py-6",
              route === "sessions" ? "max-w-[1800px]" : "max-w-6xl"
            )}
          >
            {children}
          </div>
        </div>
      </SidebarInset>
    </SidebarProvider>
  );
};
