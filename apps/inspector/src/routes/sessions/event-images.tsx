/** Images on a transcript line, shown inline.
 *
 * Events carry only where each image sits (`ImageRef`), never its base64, so a
 * screenshot-heavy session stays light. An image is fetched when the row that
 * holds it is opened: this component mounts only inside opened bodies.
 */
import { Atom, Result, useAtomValue } from "@effect-atom/atom-react";
import type {
  ImageRef,
  TimelineEvent,
} from "@workspace/core/services/sessions/schema";
import { cn } from "@workspace/ui/lib/utils";
import { Cause, Effect } from "effect";
import { useEffect, useMemo, useState } from "react";
import { apiBlob, apiPath } from "../../lib/api";
import { useTranscriptSource } from "../../lib/transcript-source";

const FRAME = "rounded-md border bg-muted/30";
const NOTE = "px-3 py-2 text-muted-foreground text-xs";

/** Natural size of an image, known once the browser has decoded it. */
const useNaturalSize = (src: string) => {
  const [size, setSize] = useState<{ h: number; w: number } | null>(null);
  useEffect(() => {
    const probe = new Image();
    probe.onload = () =>
      setSize({ h: probe.naturalHeight, w: probe.naturalWidth });
    probe.src = src;
    return () => {
      probe.onload = null;
    };
  }, [src]);
  return size;
};

/** The image itself, drawn once its size is known so the box does not jump. */
const Picture = ({
  alt,
  full,
  src,
}: {
  readonly alt: string;
  readonly full: boolean;
  readonly src: string;
}) => {
  const size = useNaturalSize(src);
  if (!size) {
    return <span className={NOTE}>Decoding image…</span>;
  }
  return (
    // biome-ignore lint/performance/noImgElement: Vite app, not Next; a blob: URL needs no optimizer.
    <img
      alt={alt}
      className={cn(
        "block h-auto w-auto",
        full ? "max-w-none" : "max-h-80 max-w-full"
      )}
      height={size.h}
      src={src}
      width={size.w}
    />
  );
};

/** An object URL for `blob`, revoked when it changes or on unmount. */
const useObjectUrl = (blob: Blob | null) => {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    if (blob === null) {
      setUrl(null);
      return;
    }
    const made = URL.createObjectURL(blob);
    setUrl(made);
    return () => URL.revokeObjectURL(made);
  }, [blob]);
  return url;
};

const EventImage = ({
  line,
  imageRef,
}: {
  readonly line: number;
  readonly imageRef: ImageRef;
}) => {
  const source = useTranscriptSource();
  const [full, setFull] = useState(false);
  const atom = useMemo(() => {
    const segments = [
      "sessions",
      source?.id ?? "",
      "images",
      line,
      imageRef.block,
      ...(imageRef.item === undefined ? [] : [imageRef.item]),
    ];
    // A missing image reads as `null`: the ref names no showable image.
    return Atom.make(
      apiBlob(apiPath(...segments), {
        parent: source?.parent ?? undefined,
      }).pipe(
        Effect.catchIf(
          (e) => e._tag === "ImageNotFoundError",
          () => Effect.succeed(null)
        )
      )
    );
  }, [source, line, imageRef]);
  const result = useAtomValue(atom);
  const blob = Result.isSuccess(result) ? result.value : null;
  const src = useObjectUrl(blob);
  if (!Result.isSuccess(result)) {
    return (
      <div className={cn(FRAME, NOTE)} data-testid="event-image-pending">
        {Result.isFailure(result) && !Cause.isInterruptedOnly(result.cause)
          ? "Image could not be read."
          : "Loading image…"}
      </div>
    );
  }
  if (result.value === null) {
    return (
      <div className={cn(FRAME, NOTE)}>
        {imageRef.mediaType} is not shown inline.
      </div>
    );
  }
  if (src === null) {
    return <span className={NOTE}>Decoding image…</span>;
  }
  return (
    <button
      className={cn(
        FRAME,
        "w-fit max-w-full cursor-zoom-in overflow-hidden p-1"
      )}
      data-testid="event-image"
      onClick={() => setFull(!full)}
      title={full ? "Shrink" : "Show full size"}
      type="button"
    >
      <Picture
        alt={`${imageRef.mediaType} from the transcript`}
        full={full}
        src={src}
      />
    </button>
  );
};

/** Every image on `e`'s line, or nothing when it has none or no source. */
export const EventImages = ({ e }: { readonly e: TimelineEvent }) => {
  const source = useTranscriptSource();
  if (!(source && e.images && e.images.length > 0)) {
    return null;
  }
  return (
    <div className="flex flex-wrap gap-2" data-testid="event-images">
      {e.images.map((ref) => (
        <EventImage
          imageRef={ref}
          key={`${ref.block}:${ref.item ?? ""}`}
          line={e.index}
        />
      ))}
    </div>
  );
};
