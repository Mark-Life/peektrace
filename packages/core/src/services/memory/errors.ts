/** Typed errors for the memory service. */
import { Data } from "effect";
import type { AgentId } from "../agents";

/**
 * A mutation was attempted for an agent whose `memory.crud` capability is not
 * `supported` (only Claude is, for now). Carries the agent + capability id.
 */
export class CapabilityUnsupportedError extends Data.TaggedError(
  "CapabilityUnsupportedError"
)<{
  readonly capabilityId: string;
  readonly agentId: AgentId;
}> {}

/** A create/update payload failed validation (bad name, type, or duplicate). */
export class MemoryValidationError extends Data.TaggedError(
  "MemoryValidationError"
)<{
  readonly reason: string;
  readonly name?: string;
}> {}

/** A referenced memory file does not exist in the project's vault. */
export class MemoryNotFoundError extends Data.TaggedError(
  "MemoryNotFoundError"
)<{
  readonly project: string;
  readonly name: string;
}> {}
