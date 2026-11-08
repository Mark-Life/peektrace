/** JSONL helpers shared by every session parser. */
type RawLine = Record<string, unknown>;

/**
 * Parse JSONL text into raw lines, skipping blanks and malformed rows.
 * `malformed` counts bad rows except the last non-blank one, which is the
 * partial write of a live session rather than damage.
 */
export const scanJsonl = (text: string) => {
  const rows: RawLine[] = [];
  let bad = 0;
  let lastBad = false;
  for (const line of text.split("\n")) {
    if (!line.trim()) {
      continue;
    }
    try {
      rows.push(JSON.parse(line) as RawLine);
      lastBad = false;
    } catch {
      bad += 1;
      lastBad = true;
    }
  }
  return { rows, malformed: lastBad ? bad - 1 : bad };
};

/** Parse JSONL text into raw lines, skipping blanks and malformed rows. */
export const parseJsonl = (text: string): RawLine[] => scanJsonl(text).rows;

/**
 * Line `n` as `parseJsonl` numbers it, parsing only that line. A line counts
 * when it looks like one JSON object; `parseJsonl` also drops a line that only
 * looks complete, so a caller that cannot confirm the result should fall back
 * to the full parse.
 */
export const jsonlLineAt = (text: string, n: number): unknown => {
  let seen = 0;
  let start = 0;
  while (start <= text.length) {
    const end = text.indexOf("\n", start);
    const line = text.slice(start, end < 0 ? text.length : end).trim();
    if (line.startsWith("{") && line.endsWith("}")) {
      if (seen === n) {
        try {
          return JSON.parse(line) as unknown;
        } catch {
          return;
        }
      }
      seen++;
    }
    if (end < 0) {
      return;
    }
    start = end + 1;
  }
  return;
};
