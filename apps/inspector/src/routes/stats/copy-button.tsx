/** Copy a line to the clipboard, and say so for a moment. Over plain http (an
 * inspector served with `--host 0.0.0.0`) `navigator.clipboard` is absent, so
 * a hidden-textarea copy is tried; when that fails too the button says so and
 * shows the text, selected, for a manual copy. */
import { Button } from "@workspace/ui/components/button";
import { CheckIcon, CopyIcon, XIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";

/** How long the button reads "Copied". */
const COPIED_MS = 1500;

type CopyState = "idle" | "copied" | "failed";

/** The pre-Clipboard-API copy: select a hidden textarea and copy it. Selecting
 * focuses the textarea, so focus goes back to where it was once it is gone. */
const legacyCopy = (text: string): boolean => {
  const before = document.activeElement;
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.opacity = "0";
  document.body.append(area);
  area.select();
  try {
    // Deprecated, yet the only copy an insecure context still allows.
    return document.execCommand("copy");
  } catch {
    return false;
  } finally {
    area.remove();
    if (before instanceof HTMLElement) {
      before.focus({ preventScroll: true });
    }
  }
};

const writeText = (text: string): Promise<boolean> =>
  navigator.clipboard === undefined
    ? Promise.resolve(legacyCopy(text))
    : navigator.clipboard.writeText(text).then(
        () => true,
        () => legacyCopy(text)
      );

export const CopyButton = ({
  label,
  testId,
  text,
}: {
  readonly label: string;
  readonly testId: string;
  readonly text: string;
}) => {
  const [state, setState] = useState<CopyState>("idle");
  const manual = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    if (state === "failed") {
      manual.current?.select();
    }
  }, [state]);
  const copy = () => {
    writeText(text).then((ok) => {
      setState(ok ? "copied" : "failed");
      if (ok) {
        setTimeout(() => setState("idle"), COPIED_MS);
      }
    });
  };
  const icon = {
    idle: <CopyIcon className="size-3" />,
    copied: <CheckIcon className="size-3" />,
    failed: <XIcon className="size-3" />,
  }[state];
  const shown = { idle: label, copied: "Copied", failed: "Copy failed" }[state];
  return (
    <span className="inline-flex shrink-0 flex-col gap-1">
      <Button
        className="h-6 shrink-0 gap-1 px-2 text-xs"
        data-testid={testId}
        onClick={copy}
        size="sm"
        title={text}
        variant="outline"
      >
        {icon}
        {shown}
      </Button>
      <span aria-live="polite" className="sr-only">
        {state === "idle" ? "" : shown}
      </span>
      {state === "failed" ? (
        <textarea
          aria-label={`${label}: select and copy`}
          className="h-16 w-64 max-w-full resize-y rounded border bg-muted p-1 font-mono text-xs"
          data-testid={`${testId}-manual`}
          readOnly
          ref={manual}
          value={text}
        />
      ) : null}
    </span>
  );
};
