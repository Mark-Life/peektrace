"use client";

import {
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@workspace/ui/components/tabs";
import { CommandBlock } from "@/components/command-block";

const UNIX_COMMAND =
  "curl -fsSL https://raw.githubusercontent.com/Mark-Life/peektrace/main/scripts/install.sh | sh";
const NPM_COMMAND = "npx peektrace serve";
const NPM_INSTALL_COMMAND = "npm install -g peektrace";
const WINDOWS_COMMAND =
  "irm https://raw.githubusercontent.com/Mark-Life/peektrace/main/scripts/install.ps1 | iex";

/**
 * Interactive install picker: two OS tabs with the native one-liner, plus an
 * npm tab, each in a copyable command block. Client-only because the tab state is stateful.
 */
export const InstallTabs = () => (
  <Tabs className="w-full gap-4" defaultValue="unix">
    <TabsList>
      <TabsTrigger value="unix">macOS / Linux</TabsTrigger>
      <TabsTrigger value="windows">Windows (PowerShell 5.1+)</TabsTrigger>
      <TabsTrigger value="npm">npm</TabsTrigger>
    </TabsList>
    <TabsContent value="unix">
      <CommandBlock command={UNIX_COMMAND} />
    </TabsContent>
    <TabsContent value="windows">
      <CommandBlock command={WINDOWS_COMMAND} />
    </TabsContent>
    <TabsContent className="flex flex-col gap-3" value="npm">
      <CommandBlock command={NPM_COMMAND} />
      <CommandBlock command={NPM_INSTALL_COMMAND} />
    </TabsContent>
  </Tabs>
);
