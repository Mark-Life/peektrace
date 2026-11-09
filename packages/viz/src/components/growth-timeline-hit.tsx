/** The pointer layer of the growth timeline: a hover guide with the turn under
 *  the pointer, and a click that reports that turn.
 *
 * The layer wraps the plot instead of covering it, so pointer events from the
 * areas and markers bubble up to it: the area tooltips still show, and a
 * marker click reports its own turn and stops there. Hover state lives here,
 * not in the chart, and the plot comes in as `children`, so moving the pointer
 * re-renders one line and one label instead of every area, dot and tick.
 * A touch screen has no hover, so there the first tap shows the guide and a
 * second tap on the same snapshot reports it.
 */
import type { TurnSnapshot } from "@workspace/core/services/sessions/schema";
import type { KeyboardEvent, MouseEvent, ReactNode } from "react";
import { useRef, useState } from "react";
import { fmtK } from "../lib/session-format";
import { clickAction, snapIndexAtX } from "../lib/timeline-hit";

/** Gap between the guide line and its label, in viewBox px. */
const LABEL_GAP = 6;
/** Label baseline below the plot top, in viewBox px. */
const LABEL_DROP = 12;

/** Props a marker needs to jump to `turn` by mouse or keyboard. The click
 *  stops at the marker, so the layer under it does not also report the
 *  snapshot nearest the pointer. */
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
  onClick: (ev: MouseEvent) => {
    ev.stopPropagation();
    onTurn(turn);
  },
  onKeyDown: (ev: KeyboardEvent) => {
    if (ev.key === "Enter" || ev.key === " ") {
      ev.preventDefault();
      onTurn(turn);
    }
  },
  role: "button",
  tabIndex: 0,
});

/** Tracks the pointer over `children` and reports the turn it clicks. */
export const TimelineHitLayer = ({
  children,
  left,
  onTurn,
  plotH,
  plotW,
  snaps,
  top,
  x,
}: {
  readonly children: ReactNode;
  readonly left: number;
  readonly onTurn: (turn: number) => void;
  readonly plotH: number;
  readonly plotW: number;
  readonly snaps: readonly TurnSnapshot[];
  readonly top: number;
  readonly x: (i: number) => number;
}) => {
  const [hover, setHover] = useState(-1);
  const area = useRef<SVGRectElement>(null);
  const touch = useRef(false);
  // The snapshot the last tap showed; a second tap on it jumps.
  const armed = useRef(-1);
  // The snapshot count is read on every event, so a live session growing
  // under a still pointer maps to the current list. -1 outside the plot.
  const indexAt = (ev: MouseEvent) => {
    const box = area.current?.getBoundingClientRect();
    if (!box || box.width <= 0) {
      return -1;
    }
    const frac = (ev.clientX - box.left) / box.width;
    if (frac < 0 || frac > 1) {
      return -1;
    }
    return snapIndexAtX({
      left,
      n: snaps.length,
      plotW,
      px: left + frac * plotW,
    });
  };
  const snap = hover >= 0 ? snaps[hover] : undefined;
  const hx = snap ? x(hover) : 0;
  const rightHalf = hx > left + plotW / 2;
  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: a pointer-only shortcut; the dumb-zone and peak markers are the keyboard path
    <g
      data-testid="timeline-hit"
      onClick={(ev) => {
        const i = indexAt(ev);
        const s = snaps[i];
        if (!s) {
          return;
        }
        if (
          clickAction({ armed: armed.current, i, touch: touch.current }) ===
          "show"
        ) {
          armed.current = i;
          setHover(i);
          return;
        }
        armed.current = -1;
        onTurn(s.turnIndex + 1);
      }}
      onPointerDown={(ev) => {
        touch.current = ev.pointerType === "touch";
      }}
      onPointerLeave={(ev) => {
        // A lifted finger leaves too; the tapped guide stays up.
        if (ev.pointerType !== "touch") {
          setHover(-1);
        }
      }}
      onPointerMove={(ev) => {
        const i = indexAt(ev);
        setHover((prev) => (prev === i ? prev : i));
      }}
    >
      <rect
        className="cursor-pointer"
        data-testid="timeline-hit-area"
        fill="transparent"
        height={plotH}
        ref={area}
        width={plotW}
        x={left}
        y={top}
      />
      {children}
      {snap ? (
        // biome-ignore lint/a11y/noAriaHiddenOnFocusable: an SVG group with no tabindex is not focusable; this hides the hover label from screen readers
        <g aria-hidden="true" pointerEvents="none">
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
    </g>
  );
};
