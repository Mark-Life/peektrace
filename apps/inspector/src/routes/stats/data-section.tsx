/** The tables behind the findings, collapsed under one toggle: retries, tool
 * errors and time as tabs. Closed by default, so each cause shows once on a
 * cold start — as a finding card. */
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@workspace/ui/components/collapsible";
import {
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@workspace/ui/components/tabs";
import type { DrillTable } from "@workspace/view-core/api-types.gen";
import { STATS_DATA } from "@workspace/view-core/constants.gen";
import { ChevronRightIcon } from "lucide-react";
import type { ReactNode } from "react";

/** Which tab a drill key opens the section on. */
export type DataTab = keyof typeof STATS_DATA.tabs;

/** The tab that shows a drill's source table. */
export const tabOf = (table: DrillTable | undefined): DataTab => {
  if (table === "fail" || table === "cluster") {
    return "fails";
  }

  return table === "time" || table === "wait" ? "time" : "retries";
};

const TAB_KEYS = [
  "retries",
  "fails",
  "time",
] as const satisfies readonly DataTab[];

const isTab = (value: string): value is DataTab =>
  TAB_KEYS.some((key) => key === value);

export const DataSection = ({
  onOpen,
  onTab,
  open,
  panels,
  tab,
}: {
  readonly onOpen: (open: boolean) => void;
  readonly onTab: (tab: DataTab) => void;
  readonly open: boolean;
  readonly panels: Readonly<Record<DataTab, ReactNode>>;
  readonly tab: DataTab;
}) => (
  <Collapsible
    className="flex flex-col gap-3"
    data-testid="stats-data"
    onOpenChange={onOpen}
    open={open}
  >
    <CollapsibleTrigger
      className="group flex items-center gap-1 self-start text-sm font-medium"
      data-testid="stats-data-toggle"
    >
      <ChevronRightIcon className="size-4 transition-transform group-data-[state=open]:rotate-90" />
      {open ? STATS_DATA.hide : STATS_DATA.title}
    </CollapsibleTrigger>
    <CollapsibleContent>
      <Tabs
        onValueChange={(value) => {
          if (isTab(value)) {
            onTab(value);
          }
        }}
        value={tab}
      >
        <TabsList>
          {TAB_KEYS.map((key) => (
            <TabsTrigger data-testid={`stats-tab-${key}`} key={key} value={key}>
              {STATS_DATA.tabs[key].title}
            </TabsTrigger>
          ))}
        </TabsList>
        {TAB_KEYS.map((key) => (
          <TabsContent
            className="mt-3 flex flex-col gap-6"
            key={key}
            value={key}
          >
            <p className="text-muted-foreground text-xs">
              {STATS_DATA.tabs[key].note}
            </p>
            {panels[key]}
          </TabsContent>
        ))}
      </Tabs>
    </CollapsibleContent>
  </Collapsible>
);
