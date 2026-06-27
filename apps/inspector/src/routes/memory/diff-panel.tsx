/** Index↔files diff panel: orphan files vs dangling index pointers.
 *
 * `orphans` = memory files present on disk but missing from MEMORY.md (Claude
 * may never load them). `dangling` = index lines pointing at a file that no
 * longer exists. A clean vault shows a single reassuring line.
 */
import type { VaultDiff } from "@workspace/core/services/memory/types";
import { FileWarningIcon, LinkIcon } from "lucide-react";

/** Render the orphans / dangling lists for one vault. */
export const DiffPanel = ({
  diff,
}: {
  readonly diff: typeof VaultDiff.Type;
}) => {
  const clean = diff.orphans.length === 0 && diff.dangling.length === 0;
  if (clean) {
    return (
      <p className="text-muted-foreground text-sm" data-testid="diff-clean">
        Index and files are in sync.
      </p>
    );
  }
  return (
    <div className="flex flex-col gap-3 text-sm" data-testid="diff-panel">
      {diff.orphans.length > 0 ? (
        <div>
          <div className="mb-1 flex items-center gap-2 font-medium text-amber-400">
            <FileWarningIcon className="size-4" />
            {diff.orphans.length} orphan file
            {diff.orphans.length === 1 ? "" : "s"} (not in index)
          </div>
          <ul className="ml-6 list-disc font-mono text-muted-foreground text-xs">
            {diff.orphans.map((o) => (
              <li key={o}>{o}</li>
            ))}
          </ul>
        </div>
      ) : null}
      {diff.dangling.length > 0 ? (
        <div>
          <div className="mb-1 flex items-center gap-2 font-medium text-red-400">
            <LinkIcon className="size-4" />
            {diff.dangling.length} dangling pointer
            {diff.dangling.length === 1 ? "" : "s"} (file missing)
          </div>
          <ul className="ml-6 list-disc font-mono text-muted-foreground text-xs">
            {diff.dangling.map((d) => (
              <li key={`${d.target}:${d.line}`}>
                {d.target} <span className="opacity-60">(line {d.line})</span>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </div>
  );
};
