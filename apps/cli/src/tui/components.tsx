/** Small reusable OpenTUI widgets shared across TUI screens.
 *
 * Kept presentational and prop-driven so each screen composes them without
 * duplicating border/label/gauge chrome. All colors come from `theme`.
 */
import type { ReactNode } from "react";
import { C, clip } from "./theme";

/** A flexbox dimension OpenTUI accepts (cells, percent, or auto). */
type Dimension = number | "auto" | `${number}%`;

/** Cap for an error message rendered on a single line. */
const ERROR_MSG_MAX = 200;

/** A bordered panel with an optional title; border brightens when `focused`. */
export const Panel = ({
  title,
  focused = false,
  children,
  flexGrow,
  width,
  height,
  padding = 1,
}: {
  readonly title?: string;
  readonly focused?: boolean;
  readonly children?: ReactNode;
  readonly flexGrow?: number;
  readonly width?: Dimension;
  readonly height?: Dimension;
  readonly padding?: number;
}) => (
  <box
    {...(title === undefined ? {} : { title })}
    borderColor={focused ? C.borderActive : C.border}
    borderStyle="rounded"
    style={{
      flexDirection: "column",
      padding,
      ...(flexGrow === undefined ? {} : { flexGrow }),
      ...(width === undefined ? {} : { width }),
      ...(height === undefined ? {} : { height }),
      minHeight: 0,
    }}
    titleColor={focused ? C.primary : C.textDim}
  >
    {children}
  </box>
);

/** A compact bracketed label in an accent color, e.g. `[Claude]`. */
export const Badge = ({
  label,
  color = C.textDim,
}: {
  readonly label: string;
  readonly color?: string;
}) => <text fg={color}>{`[${label}]`}</text>;

/** Dim single-line placeholder while a query loads. */
export const Loading = ({
  label = "Loading…",
}: {
  readonly label?: string;
}) => <text fg={C.textDim}>{label}</text>;

/** Red single-line failure message. */
export const ErrorLine = ({ error }: { readonly error: Error }) => (
  <text fg={C.bad}>{`✗ ${clip(error.message, ERROR_MSG_MAX)}`}</text>
);

/** Dim empty-state message. */
export const Empty = ({ label }: { readonly label: string }) => (
  <text fg={C.textFaint}>{label}</text>
);

/** A `label: value` row with a dim label and normal-weight value. */
export const Field = ({
  label,
  value,
  valueColor = C.text,
  labelWidth = 16,
}: {
  readonly label: string;
  readonly value: string;
  readonly valueColor?: string;
  readonly labelWidth?: number;
}) => (
  <box style={{ flexDirection: "row" }}>
    <text fg={C.textDim}>{label.padEnd(labelWidth)}</text>
    <text fg={valueColor}>{value}</text>
  </box>
);

/** One colored segment of a stacked bar. */
export interface BarSegment {
  readonly color: string;
  readonly weight: number;
}

/**
 * A single-row stacked bar: each segment claims a share of `width` cells
 * proportional to its `weight`. Zero-total renders an empty track.
 */
export const StackedBar = ({
  segments,
  width,
}: {
  readonly segments: readonly BarSegment[];
  readonly width: number;
}) => {
  const total = segments.reduce((sum, s) => sum + Math.max(0, s.weight), 0);
  const cells: { color: string; n: number; offset: number }[] = [];
  let used = 0;
  if (total > 0) {
    for (const seg of segments) {
      const n = Math.round((Math.max(0, seg.weight) / total) * width);
      if (n > 0) {
        cells.push({ color: seg.color, n, offset: used });
        used += n;
      }
    }
  }
  const rest = width - used;
  return (
    <box style={{ flexDirection: "row" }}>
      {cells.map((cell) => (
        <text
          bg={cell.color}
          fg={cell.color}
          key={`${cell.offset}:${cell.color}`}
        >
          {"█".repeat(cell.n)}
        </text>
      ))}
      {rest > 0 ? <text fg={C.border}>{"░".repeat(rest)}</text> : null}
    </box>
  );
};

/** A small color swatch (used in legends / budget tables). */
export const Swatch = ({ color }: { readonly color: string }) => (
  <text fg={color}>■</text>
);

/** Footer key-hint bar: `key label` pairs separated by dots. */
export const KeyHints = ({
  hints,
}: {
  readonly hints: readonly (readonly [string, string])[];
}) => (
  <box style={{ flexDirection: "row", gap: 2 }}>
    {hints.map(([key, label]) => (
      <box key={`${key}:${label}`} style={{ flexDirection: "row" }}>
        <text fg={C.accent}>{key}</text>
        <text fg={C.textFaint}>{` ${label}`}</text>
      </box>
    ))}
  </box>
);

/**
 * A clickable bordered container. The whole card is a mouse target (`onSelect`
 * fires on click) and brightens its border when `selected`. Screens compose the
 * inner rows as children.
 */
export const Card = ({
  selected = false,
  onSelect,
  children,
  title,
}: {
  readonly selected?: boolean;
  readonly onSelect?: () => void;
  readonly children?: ReactNode;
  readonly title?: string;
}) => (
  // biome-ignore lint/a11y/noStaticElementInteractions: OpenTUI box is the click target; no DOM roles apply
  <box
    {...(title === undefined ? {} : { title })}
    {...(onSelect === undefined ? {} : { onMouseDown: onSelect })}
    borderColor={selected ? C.borderActive : C.border}
    borderStyle="rounded"
    style={{
      flexDirection: "column",
      paddingLeft: 1,
      paddingRight: 1,
      ...(selected ? { backgroundColor: C.panelSel } : {}),
    }}
    titleColor={selected ? C.primary : C.textDim}
  >
    {children}
  </box>
);

/** A clickable inline label — a poor-man's button for toolbars. */
export const TextButton = ({
  label,
  onPress,
  active = false,
}: {
  readonly label: string;
  readonly onPress: () => void;
  readonly active?: boolean;
}) => (
  // biome-ignore lint/a11y/noStaticElementInteractions: OpenTUI box is the click target; no DOM roles apply
  <box onMouseDown={onPress} style={{ flexDirection: "row" }}>
    <text
      bg={active ? C.primary : C.panelSel}
      fg={active ? C.onPrimary : C.accent}
    >
      {` ${label} `}
    </text>
  </box>
);
