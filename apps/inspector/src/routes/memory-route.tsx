/** Memory section (Phase 7) — the cross-project explorer + full CRUD.
 *
 * Default = all-projects explorer (`memory.allVaults`): project overview, then
 * each project's forensic surface (budget gauge, type donut, browse table,
 * index↔files diff, link graph) with create/edit/delete, capability-gated by the
 * live matrix. The heavy lifting lives in `./memory/memory-explorer`.
 */
import { MemoryExplorer } from "./memory/memory-explorer";

/** Memory section route. */
export const MemoryRoute = () => <MemoryExplorer />;
