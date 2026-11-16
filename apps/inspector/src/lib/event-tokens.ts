/** An event's size and its label. */
import type { TimelineEvent } from "@workspace/view-core/api-types.gen";

/** An event's best known size: measured when usage pins it down, else estimated. */
export const eventTokens = (
  e: Pick<TimelineEvent, "tokensEst" | "tokensMeasured">
) => ({
  measured: e.tokensMeasured !== undefined,
  tokens: e.tokensMeasured ?? e.tokensEst,
});

/** Size label for an event: `~` marks a chars/4 estimate, none a measured size. */
export const tokenLabel = (
  e: Pick<TimelineEvent, "tokensEst" | "tokensMeasured">,
  format: (n: number) => string
) => {
  const { tokens, measured } = eventTokens(e);

  return `${measured ? "" : "~"}${format(tokens)}`;
};
