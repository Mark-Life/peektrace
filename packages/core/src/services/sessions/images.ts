/** Image blocks in transcripts: shown as a short marker, never as base64. */

type Raw = Record<string, unknown>;

const isRecord = (v: unknown): v is Raw =>
  typeof v === "object" && v !== null && !Array.isArray(v);

/** Media type of a Claude (`source.media_type`) or Pi (`mimeType`) image block. */
const mediaType = (block: Raw): string => {
  const source = isRecord(block.source) ? block.source : {};
  const t = source.media_type ?? block.mimeType ?? block.media_type;
  return typeof t === "string" ? t : "image";
};

/** Leading text of every image marker. */
export const IMAGE_MARKER = "[image: ";

/** Marker text for an image block, e.g. `[image: image/png]`. */
export const imageMarker = (block: Raw) =>
  `${IMAGE_MARKER}${mediaType(block)}]`;

/** Whether a content block is an image. */
export const isImageBlock = (block: unknown) =>
  isRecord(block) && block.type === "image";

/** Replace base64 payloads in a content array so bodies and token estimates stay honest. */
export const omitImageData = (content: unknown): unknown =>
  Array.isArray(content)
    ? content.map((b) =>
        isRecord(b) && isImageBlock(b)
          ? { type: "text", text: imageMarker(b) }
          : b
      )
    : content;
