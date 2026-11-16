/** A minimal terminal screen for `bench-tui.ts`: enough of VT100 to read
 * the text a TUI draws (cursor moves, erases, printable cells). Colors and
 * modes are skipped; OSC, DCS and APC strings are dropped. */

const ESC = 0x1b;
const BEL = "\x07";
const TAB = 8;
const PRIVATE_CSI = /^[<=>?]/;
const CSI_FINAL = /[\x40-\x7e]/;
const CSI_INTERMEDIATE = /[ -/]+$/;
/** Introducers of strings that run to BEL or ST: OSC, DCS, APC, PM, SOS. */
const STRING_KINDS = "]P_^X";
const CHARSET_KINDS = "()*+";

export const createScreen = (cols: number, rows: number) => {
  const blank = () => Array.from({ length: cols }, () => " ");
  let grid = Array.from({ length: rows }, blank);
  let x = 0;
  let y = 0;
  const decoder = new TextDecoder();
  /** Escape bytes cut off at the end of the last chunk. */
  let pending = "";

  const clampY = (v: number) => Math.max(0, Math.min(rows - 1, v));
  const clampX = (v: number) => Math.max(0, Math.min(cols - 1, v));
  const lineFeed = () => {
    if (y === rows - 1) {
      grid = [...grid.slice(1), blank()];
    } else {
      y++;
    }
  };
  const put = (ch: string) => {
    const w = Bun.stringWidth(ch);
    if (w === 0) {
      return;
    }
    if (x + w > cols) {
      x = 0;
      lineFeed();
    }
    grid[y][x] = ch;
    if (w === 2 && x + 1 < cols) {
      grid[y][x + 1] = "";
    }
    x += w;
  };
  const eraseLine = (mode: number) => {
    const span: Record<number, [number, number]> = {
      1: [0, x + 1],
      2: [0, cols],
    };
    const [from, to] = span[mode] ?? [x, cols];
    for (let i = from; i < to; i++) {
      grid[y][i] = " ";
    }
  };
  const eraseDisplay = (mode: number) => {
    if (mode >= 2) {
      grid = Array.from({ length: rows }, blank);
      return;
    }
    eraseLine(mode);
    const others =
      mode === 1
        ? Array.from({ length: y }, (_, i) => i)
        : Array.from({ length: rows - y - 1 }, (_, i) => y + 1 + i);
    for (const r of others) {
      grid[r] = blank();
    }
  };
  const csi = (body: string, final: string) => {
    if (PRIVATE_CSI.test(body)) {
      return;
    }
    const ps = body.split(";").map((p) => Number.parseInt(p, 10));
    const n = (i: number, d: number) =>
      Number.isNaN(ps[i]) || ps[i] === 0 ? d : ps[i];
    switch (final) {
      case "H":
      case "f":
        y = clampY(n(0, 1) - 1);
        x = clampX(n(1, 1) - 1);
        break;
      case "A":
        y = clampY(y - n(0, 1));
        break;
      case "B":
        y = clampY(y + n(0, 1));
        break;
      case "C":
        x = clampX(x + n(0, 1));
        break;
      case "D":
        x = clampX(x - n(0, 1));
        break;
      case "G":
        x = clampX(n(0, 1) - 1);
        break;
      case "d":
        y = clampY(n(0, 1) - 1);
        break;
      case "K":
        eraseLine(Number.isNaN(ps[0]) ? 0 : ps[0]);
        break;
      case "J":
        eraseDisplay(Number.isNaN(ps[0]) ? 0 : ps[0]);
        break;
      case "X":
        for (let i = 0; i < n(0, 1) && x + i < cols; i++) {
          grid[y][x + i] = " ";
        }
        break;
      default:
        break;
    }
  };

  /** End of the CSI at `s[i]`, or -1 when it is cut off. */
  const csiEnd = (s: string, i: number) => {
    let j = i + 2;
    while (j < s.length && !CSI_FINAL.test(s[j])) {
      j++;
    }
    if (j >= s.length) {
      return -1;
    }
    csi(s.slice(i + 2, j).replace(CSI_INTERMEDIATE, ""), s[j]);
    return j + 1;
  };
  /** End of the BEL- or ST-terminated string at `s[i]`, or -1. */
  const stringEnd = (s: string, i: number) => {
    for (let j = i + 2; j < s.length; j++) {
      if (s[j] === BEL) {
        return j + 1;
      }
      if (s.charCodeAt(j) === ESC && s[j + 1] === "\\") {
        return j + 2;
      }
    }
    return -1;
  };
  /** Consumes one escape sequence at `s[i]`; returns the index after it,
   * or -1 when the sequence is cut off. */
  const skipEscape = (s: string, i: number) => {
    const kind = s[i + 1];
    if (kind === undefined) {
      return -1;
    }
    if (kind === "[") {
      return csiEnd(s, i);
    }
    if (STRING_KINDS.includes(kind)) {
      return stringEnd(s, i);
    }
    if (CHARSET_KINDS.includes(kind)) {
      return i + 2 < s.length ? i + 3 : -1;
    }
    return i + 2;
  };
  const controls: Record<string, () => void> = {
    "\r": () => {
      x = 0;
    },
    "\n": lineFeed,
    "\b": () => {
      x = Math.max(0, x - 1);
    },
    "\t": () => {
      x = clampX((Math.floor(x / TAB) + 1) * TAB);
    },
  };

  const write = (bytes: Uint8Array) => {
    const s = pending + decoder.decode(bytes, { stream: true });
    pending = "";
    let i = 0;
    while (i < s.length) {
      if (s.charCodeAt(i) === ESC) {
        const next = skipEscape(s, i);
        if (next < 0) {
          pending = s.slice(i);
          return;
        }
        i = next;
      } else if (s[i] < " ") {
        controls[s[i]]?.();
        i++;
      } else {
        const ch = String.fromCodePoint(s.codePointAt(i) ?? 0);
        put(ch);
        i += ch.length;
      }
    }
  };

  const text = () => grid.map((r) => r.join("")).join("\n");
  return { text, write };
};
