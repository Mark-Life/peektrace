import type { FileSystem } from "@effect/platform";
import { Effect, Option } from "effect";
import type { SessionText } from "./sessions/opencode/reader";

/** File size and mtime, as read by `fs.stat`. */
export type FileStamp = Pick<SessionText, "mtimeMs" | "sizeBytes">;

export const statFileStamp = (fs: FileSystem.FileSystem, path: string) =>
  fs.stat(path).pipe(
    Effect.map(
      (info): FileStamp => ({
        sizeBytes: Number(info.size),
        mtimeMs: Option.match(info.mtime, {
          onNone: () => 0,
          onSome: (d) => d.getTime(),
        }),
      })
    )
  );
