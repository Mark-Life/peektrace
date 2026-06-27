/** Public surface of the memory slice (Phase 3). */
export {
  CapabilityUnsupportedError,
  MemoryNotFoundError,
  MemoryValidationError,
} from "./errors";
export {
  MemoryService,
  MemoryServiceLive,
  type MemoryServiceShape,
} from "./service";
export * from "./types";
