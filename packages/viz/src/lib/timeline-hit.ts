/** Which snapshot a pointer over the growth timeline points at.
 *
 * `px` is the pointer x in viewBox units, `left` and `plotW` the plot's left
 * edge and width, `n` the snapshot count. Snapshots sit evenly from the left
 * edge to the right one (a single snapshot sits in the middle), so the nearest
 * one is a rounded fraction, clamped to the plot. -1 when there is none.
 */
export const snapIndexAtX = ({
  left,
  n,
  plotW,
  px,
}: {
  readonly left: number;
  readonly n: number;
  readonly plotW: number;
  readonly px: number;
}) => {
  if (n <= 0) {
    return -1;
  }
  if (n === 1 || plotW <= 0) {
    return 0;
  }
  const frac = Math.min(Math.max((px - left) / plotW, 0), 1);
  return Math.round(frac * (n - 1));
};

/** What a click on snapshot `i` does. A mouse jumps at once; a touch shows
 *  the guide first (there is no hover), and jumps on a second tap of the same
 *  snapshot, the one `armed` holds. */
export const clickAction = ({
  armed,
  i,
  touch,
}: {
  readonly armed: number;
  readonly i: number;
  readonly touch: boolean;
}) => (touch && armed !== i ? "show" : "jump");
