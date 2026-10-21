/** "Copy prompt": hand an agent the data behind a row, or the whole report, as
 * a ready investigation prompt. The text is core's, so it matches
 * `peektrace stats prompt` word for word. */
import { CopyButton } from "./copy-button";

export const PromptButton = ({
  subject,
  testId,
  text,
}: {
  /** What the prompt is about, read out by a screen reader. */
  readonly subject: string;
  readonly testId: string;
  /** Null when the row is not in the report: no button. */
  readonly text: string | null;
}) =>
  text === null ? null : (
    <CopyButton
      describe={`Copy an agent prompt to investigate ${subject}`}
      label="Copy prompt"
      testId={testId}
      text={text}
    />
  );
