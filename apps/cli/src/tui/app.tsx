/** Root of the terminal UI: top nav + active screen + footer status bar.
 *
 * Mirrors the web inspector's shell — the same sections (Sessions, Memory,
 * Capabilities, Settings, Stats), a dark dense frame, and a footer that surfaces
 * the sibling web server URL (both surfaces are live at once). Section switching
 * is `1`–`5`; `q` / `Ctrl+C` quit. Each screen owns its own internal navigation.
 */
import { useKeyboard } from "@opentui/react";
import { useState } from "react";
import { KeyHints } from "./components";
import { BridgeProvider, type TuiBridge, WatchProvider } from "./runtime";
import { CapabilitiesScreen } from "./screens/capabilities";
import { MemoryScreen } from "./screens/memory";
import { SessionsScreen } from "./screens/sessions";
import { SettingsScreen } from "./screens/settings";
import { StatsScreen } from "./screens/stats";
import { C } from "./theme";
import { TypingProvider, useTyping } from "./typing";

type SectionId = "sessions" | "memory" | "capabilities" | "settings" | "stats";

interface Section {
  readonly hint: string;
  readonly id: SectionId;
  readonly label: string;
}

const SECTIONS: readonly Section[] = [
  { id: "sessions", label: "Sessions", hint: "Context debug" },
  { id: "memory", label: "Memory", hint: "All projects" },
  { id: "capabilities", label: "Capabilities", hint: "Support matrix" },
  { id: "settings", label: "Settings", hint: "Agent roots" },
  { id: "stats", label: "Stats", hint: "What fails, what costs time" },
];

/** Top brand + section tabs, active tab tinted and numbered. */
const Header = ({ active }: { readonly active: SectionId }) => (
  <box
    style={{
      flexDirection: "row",
      height: 1,
      gap: 2,
      paddingLeft: 1,
      paddingRight: 1,
    }}
  >
    <text attributes={1} fg={C.primary}>
      Peektrace
    </text>
    <box style={{ flexDirection: "row", gap: 2, flexGrow: 1 }}>
      {SECTIONS.map((s, i) => {
        const label = ` ${i + 1} ${s.label} `;
        return (
          <box key={s.id} style={{ flexDirection: "row" }}>
            {s.id === active ? (
              <text bg={C.primary} fg={C.onPrimary}>
                {label}
              </text>
            ) : (
              <text fg={C.textFaint}>{label}</text>
            )}
          </box>
        );
      })}
    </box>
    <text fg={C.textFaint}>loopback only</text>
  </box>
);

/** Bottom status bar: live web URL on the left, global key hints on the right. */
const Footer = ({ serverUrl }: { readonly serverUrl: string }) => (
  <box
    style={{
      flexDirection: "row",
      height: 1,
      paddingLeft: 1,
      paddingRight: 1,
      justifyContent: "space-between",
    }}
  >
    <box style={{ flexDirection: "row" }}>
      <text fg={C.accent}>web ▸ </text>
      <text fg={C.textDim}>{serverUrl}</text>
    </box>
    <KeyHints
      hints={[
        ["1-5", "section"],
        ["q", "quit"],
      ]}
    />
  </box>
);

/** Render the screen for the active section. */
const Screen = ({ active }: { readonly active: SectionId }) => {
  switch (active) {
    case "sessions":
      return <SessionsScreen />;
    case "memory":
      return <MemoryScreen />;
    case "capabilities":
      return <CapabilitiesScreen />;
    case "settings":
      return <SettingsScreen />;
    case "stats":
      return <StatsScreen />;
    default:
      return null;
  }
};

/** The shell body — depends on the typing flag, so it lives inside the provider. */
const Shell = ({
  serverUrl,
  onQuit,
}: {
  readonly serverUrl: string;
  readonly onQuit: () => void;
}) => {
  const { typing } = useTyping();
  const active = useActiveSection(typing, onQuit);
  return (
    <box
      style={{
        flexDirection: "column",
        width: "100%",
        height: "100%",
        backgroundColor: C.bg,
      }}
    >
      <Header active={active} />
      <box
        style={{ flexGrow: 1, minHeight: 0, paddingLeft: 1, paddingRight: 1 }}
      >
        <Screen active={active} />
      </box>
      <Footer serverUrl={serverUrl} />
    </box>
  );
};

/** Owns the active section + the global keymap (quit / section switch). */
const useActiveSection = (typing: boolean, onQuit: () => void): SectionId => {
  const [active, setActive] = useState<SectionId>("sessions");
  useKeyboard((key) => {
    if (key.ctrl && key.name === "c") {
      onQuit();
      return;
    }
    if (typing) {
      return;
    }
    if (key.name === "q") {
      onQuit();
      return;
    }
    const n = Number.parseInt(key.name, 10);
    if (Number.isInteger(n) && n >= 1 && n <= SECTIONS.length) {
      const next = SECTIONS[n - 1];
      if (next) {
        setActive(next.id);
      }
    }
  });
  return active;
};

/** Root component: providers + shell. */
export const App = ({
  bridge,
  onQuit,
}: {
  readonly bridge: TuiBridge;
  readonly onQuit: () => void;
}) => (
  <BridgeProvider bridge={bridge}>
    <WatchProvider>
      <TypingProvider>
        <Shell onQuit={onQuit} serverUrl={bridge.serverUrl} />
      </TypingProvider>
    </WatchProvider>
  </BridgeProvider>
);
