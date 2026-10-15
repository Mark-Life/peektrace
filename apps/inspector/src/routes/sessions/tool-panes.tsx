/** Tool payload panes, shared by the chat card and the table row: a diff, a
 *  highlighted code block, or Markdown for free text. */
import {
  ToolDiff,
  ToolInput,
  ToolOutput,
  ToolProse,
} from "@workspace/ui/components/ai-elements/tool";
import type { ResultPane, ToolPane } from "../../lib/tool-event";

/** One pane of a tool call's arguments. */
export const ToolPaneView = ({ pane }: { readonly pane: ToolPane }) => {
  if (pane.type === "diff") {
    return (
      <ToolDiff
        diff={pane.diff}
        label={pane.label}
        language={pane.language}
        newCode={pane.newCode}
        oldCode={pane.oldCode}
      />
    );
  }
  if (pane.type === "prose") {
    return <ToolProse label={pane.label} text={pane.text} />;
  }
  return (
    <ToolInput code={pane.code} label={pane.label} language={pane.language} />
  );
};

/** A tool result, titled "Error" when the tool reported one. */
export const ToolResultView = ({
  isError,
  pane,
}: {
  readonly isError: boolean;
  readonly pane: ResultPane;
}) =>
  pane.type === "prose" ? (
    <ToolProse
      isError={isError}
      label={isError ? "Error" : "Result"}
      text={pane.text}
    />
  ) : (
    <ToolOutput code={pane.code} isError={isError} language={pane.language} />
  );
