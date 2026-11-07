/** Images on a transcript line, shown inline.
 *
 * Events carry only where each image sits (`ImageRef`), never its base64, so a
 * screenshot-heavy session stays light. An image is fetched when the row that
 * holds it is opened: this component mounts only inside opened bodies.
 */
import { Result, useAtomValue } from "@effect-atom/atom-react";
import type {
  ImageRef,
  TimelineEvent,
} from "@workspace/core/services/sessions/schema";
import { cn } from "@workspace/ui/lib/utils";
import { useEffect, useMemo, useState } from "react";
import { PeektraceClient } from "../../lib/client";
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
    // biome-ignore lint/performance/noImgElement: Vite app, not Next; a data: URL needs no optimizer.
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

const EventImage = ({
  line,
  imageRef,
}: {
  readonly line: number;
  readonly imageRef: ImageRef;
}) => {
  const source = useTranscriptSource();
  const [full, setFull] = useState(false);
  const atom = useMemo(
    () =>
      PeektraceClient.query("sessions.image", {
        id: source?.id ?? "",
        line,
        block: imageRef.block,
        ...(source?.parent ? { parent: source.parent } : {}),
        ...(imageRef.item === undefined ? {} : { item: imageRef.item }),
      }),
    [source, line, imageRef]
  );
  const result = useAtomValue(atom);
  if (!Result.isSuccess(result)) {
    return (
      <div className={cn(FRAME, NOTE)} data-testid="event-image-pending">
        {Result.isFailure(result)
          ? "Image could not be read."
          : "Loading image…"}
      </div>
    );
  }
  const image = result.value;
  if (image === null) {
    return (
      <div className={cn(FRAME, NOTE)}>
        {imageRef.mediaType} is not shown inline.
      </div>
    );
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
        src={`data:${image.mediaType};base64,${image.data}`}
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
