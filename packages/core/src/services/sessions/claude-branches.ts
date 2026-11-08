/**
 * Claude Code transcripts are trees linked by `parentUuid`. A rewind slices
 * the conversation before a prompt, so the next prompt becomes a sibling of
 * the rewound one. This finds the rows under such abandoned prompts.
 */
type RawLine = Record<string, unknown>;

const str = (v: unknown) => (typeof v === "string" ? v : null);

const hasToolResult = (o: RawLine) => {
  const content = (o.message as RawLine | undefined)?.content;
  return (
    Array.isArray(content) &&
    content.some((b) => (b as RawLine | null)?.type === "tool_result")
  );
};

/** A prompt the user typed: tool results, meta and summaries are not prompts. */
const isPrompt = (o: RawLine) =>
  o.type === "user" &&
  str(o.uuid) !== null &&
  str(o.parentUuid) !== null &&
  o.isSidechain !== true &&
  o.isMeta !== true &&
  o.isCompactSummary !== true &&
  o.sourceToolAssistantUUID === undefined &&
  o.toolUseResult === undefined &&
  !hasToolResult(o);

/** Prompts sharing a parent, distinct uuids in file order; all but the last. */
const forkRoots = (lines: readonly RawLine[]) => {
  const groups = new Map<string, string[]>();
  for (const o of lines) {
    if (isPrompt(o)) {
      const parent = o.parentUuid as string;
      const uuid = o.uuid as string;
      const group = groups.get(parent) ?? [];
      if (!group.includes(uuid)) {
        group.push(uuid);
      }
      groups.set(parent, group);
    }
  }
  return [...groups.values()].flatMap((g) => g.slice(0, -1));
};

/** Child line indexes per uuid, over `parentUuid` and `logicalParentUuid`. */
const childrenOf = (lines: readonly RawLine[]) => {
  const children = new Map<string, number[]>();
  const add = (parent: string | null, index: number) => {
    if (parent !== null) {
      children.set(parent, [...(children.get(parent) ?? []), index]);
    }
  };
  lines.forEach((o, index) => {
    if (str(o.uuid) !== null && o.isSidechain !== true) {
      add(str(o.parentUuid), index);
      add(str(o.logicalParentUuid), index);
    }
  });
  return children;
};

/** Line indexes per uuid; a duplicated row maps to several. */
const indexesOf = (lines: readonly RawLine[]) => {
  const byUuid = new Map<string, number[]>();
  lines.forEach((o, index) => {
    const uuid = str(o.uuid);
    if (uuid !== null && o.isSidechain !== true) {
      byUuid.set(uuid, [...(byUuid.get(uuid) ?? []), index]);
    }
  });
  return byUuid;
};

const lastMainTurnRow = (lines: readonly RawLine[]) => {
  for (let i = lines.length - 1; i >= 0; i--) {
    const o = lines[i];
    if ((o?.type === "user" || o?.type === "assistant") && !o.isSidechain) {
      return i;
    }
  }
  return -1;
};

/**
 * Line indexes on branches abandoned by a rewind. Empty means keep every row:
 * no fork was found, or the live leaf came out dead and the tree is not
 * trusted. A rewind to the very first prompt (null parent) is not detected.
 */
export const deadBranchRows = (
  lines: readonly RawLine[]
): ReadonlySet<number> => {
  const roots = forkRoots(lines);
  if (roots.length === 0) {
    return new Set();
  }
  const children = childrenOf(lines);
  const byUuid = indexesOf(lines);
  const dead = new Set<number>();
  const visited = new Set<string>();
  const stack = [...roots];
  while (stack.length > 0) {
    const uuid = stack.pop() as string;
    if (!visited.has(uuid)) {
      visited.add(uuid);
      for (const i of byUuid.get(uuid) ?? []) {
        dead.add(i);
      }
      for (const i of children.get(uuid) ?? []) {
        dead.add(i);
        const child = str(lines[i]?.uuid);
        if (child !== null) {
          stack.push(child);
        }
      }
    }
  }
  return dead.has(lastMainTurnRow(lines)) ? new Set() : dead;
};
