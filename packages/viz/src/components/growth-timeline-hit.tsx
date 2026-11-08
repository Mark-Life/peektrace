/** The pointer layer of the growth timeline: a hover guide with the turn under
 *  the pointer, and a click that reports that turn.
 *
 * Hover state lives here, not in the chart, so moving the pointer re-renders
 * one line and one label instead of every area, dot and tick.
 */
import type { TurnSnapshot } from "@workspace/core/services/sessions/schema";
import type { KeyboardEvent, MouseEvent } from "react";
import { useState } from "react";
import { fmtK } from "../lib/session-format";
import { turnAtX } from "../lib/timeline-hit";

/** Gap between the guide line and its label, in viewBox px. */
const LABEL_GAP = 6;
/** Label baseline below the plot top, in viewBox px. */
const LABEL_DROP = 12;

/** Props a keyboard-reachable marker needs to jump to `turn`. */
export const markerProps = ({
  label,
  onTurn,
  turn,
}: {
  readonly label: string;
  readonly onTurn: (turn: number) => void;
  readonly turn: number;
}) => ({
  "aria-label": label,
  className: "cursor-pointer",
  onClick: () => onTurn(turn),
  onKeyDown: (ev: KeyboardEvent) => {
    if (ev.key === "Enter" || ev.key === " ") {
      ev.preventDefault();
      onTurn(turn);
    }
  },
  role: "button",
  tabIndex: 0,
});

/** A transparent rect over the plot that tracks the pointer. */
export const TimelineHitLayer = ({
  left,
  onTurn,
  plotH,
  plotW,
  snaps,
  top,
  x,
}: {
  readonly left: number;
  readonly onTurn: (turn: number) => void;
  readonly plotH: number;
  readonly plotW: number;
  readonly snaps: readonly TurnSnapshot[];
  readonly top: number;
  readonly x: (i: number) => number;
}) => {
  const [hover, setHover] = useState(-1);
  // The snapshot count is read on every event, so a live session growing
  // under a still pointer maps to the current list.
  const indexAt = (ev: MouseEvent<SVGRectElement>) => {
    const box = ev.currentTarget.getBoundingClientRect();
    if (box.width <= 0) {
      return -1;
    }
    const px = left + ((ev.clientX - box.left) / box.width) * plotW;
    return turnAtX({ left, n: snaps.length, plotW, px });
  };
  const snap = hover >= 0 ? snaps[hover] : undefined;
  const hx = snap ? x(hover) : 0;
  const rightHalf = hx > left + plotW / 2;
  return (
    <g data-testid="timeline-hit">
      {snap ? (
        <g pointerEvents="none">
          <line
            stroke="currentColor"
            strokeOpacity="0.5"
            x1={hx.toFixed(1)}
            x2={hx.toFixed(1)}
            y1={top}
            y2={top + plotH}
          />
          <text
            className="fill-foreground"
            fontSize="11"
            textAnchor={rightHalf ? "end" : "start"}
            x={(rightHalf ? hx - LABEL_GAP : hx + LABEL_GAP).toFixed(1)}
            y={top + LABEL_DROP}
          >
            turn {snap.turnIndex + 1} · ctx {fmtK(snap.ctx)}
          </text>
        </g>
      ) : null}
      {/* biome-ignore lint/a11y/noStaticElementInteractions: a pointer-only shortcut; the dumb-zone and peak markers are the keyboard path */}
      <rect
        className="cursor-pointer"
        data-testid="timeline-hit-area"
        fill="transparent"
        height={plotH}
        onClick={(ev) => {
          const s = snaps[indexAt(ev)];
          if (s) {
            onTurn(s.turnIndex + 1);
          }
        }}
        onPointerLeave={() => setHover(-1)}
        onPointerMove={(ev) => {
          const i = indexAt(ev);
          setHover((prev) => (prev === i ? prev : i));
        }}
        width={plotW}
        x={left}
        y={top}
      />
    </g>
  );
};
