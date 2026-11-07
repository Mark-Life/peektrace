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

/** Raster types the inspector may show inline. SVG is left out: it is markup. */
const SHOWABLE = new Set([
  "image/png",
  "image/jpeg",
  "image/gif",
  "image/webp",
]);

const BASE64 = /^[A-Za-z0-9+/]+={0,2}$/;

/** Where an image sits in a Claude user line: content block `block`, and item
 *  `item` inside it when the block is a tool result. */
export const imageRef = (args: {
  readonly block: Raw;
  readonly blockIndex: number;
  readonly item?: number;
}) => ({
  block: args.blockIndex,
  mediaType: mediaType(args.block),
  ...(args.item === undefined ? {} : { item: args.item }),
});

/** Refs to every image inside a tool result's content, in order. */
export const toolResultImageRefs = (blockIndex: number, content: unknown) =>
  Array.isArray(content)
    ? content.flatMap((b, item) =>
        isRecord(b) && isImageBlock(b)
          ? [imageRef({ block: b, blockIndex, item })]
          : []
      )
    : [];

/** Item `item` of a tool result block's content array. */
const itemOf = (block: unknown, item: number) =>
  isRecord(block) && Array.isArray(block.content)
    ? block.content[item]
    : undefined;

/**
 * The base64 payload of one image in a parsed transcript line, or undefined
 * when the ref points at anything else or at a type the inspector won't show.
 */
export const imageAt = (args: {
  readonly line: unknown;
  readonly block: number;
  readonly item?: number | undefined;
}): { readonly data: string; readonly mediaType: string } | undefined => {
  const message = isRecord(args.line) ? args.line.message : undefined;
  const content = isRecord(message) ? message.content : undefined;
  const outer = Array.isArray(content) ? content[args.block] : undefined;
  const inner = args.item === undefined ? outer : itemOf(outer, args.item);
  if (!(isRecord(inner) && isImageBlock(inner))) {
    return;
  }
  const source = isRecord(inner.source) ? inner.source : {};
  const type = mediaType(inner);
  const data = source.data;
  if (
    source.type !== "base64" ||
    typeof data !== "string" ||
    !SHOWABLE.has(type) ||
    !BASE64.test(data)
  ) {
    return;
  }
  return { data, mediaType: type };
};
