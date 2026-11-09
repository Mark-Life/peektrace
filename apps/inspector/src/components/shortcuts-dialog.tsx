/** The `?` sheet: every shortcut, read from the table that binds them. */
import { Button } from "@workspace/ui/components/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@workspace/ui/components/dialog";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@workspace/ui/components/tooltip";
import { KeyboardIcon } from "lucide-react";
import { ShortcutKeys } from "../lib/shortcut-keys";
import {
  ariaKeys,
  isMacPlatform,
  SHORTCUTS,
  type Shortcut,
  useShortcut,
} from "../lib/shortcuts";

const GROUPS: readonly Shortcut["group"][] = [
  "Sessions",
  "Transcript",
  "General",
];

/** Where a group's keys work, when that is narrower than the whole app. */
const GROUP_NOTE: Partial<Record<Shortcut["group"], string>> = {
  Sessions: "Moving and opening work while a session row has focus.",
  Transcript:
    "In an open transcript. The table view moves by turn only when sorted by Turn.",
};

/** The grouped rows, also rendered on their own in tests. */
export const ShortcutsList = ({ mac }: { readonly mac: boolean }) => (
  <div className="flex flex-col gap-4" data-testid="shortcuts-list">
    {GROUPS.map((group) => (
      <section aria-label={group} className="flex flex-col gap-1" key={group}>
        <h3 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
          {group}
        </h3>
        {GROUP_NOTE[group] ? (
          <p className="text-muted-foreground text-xs">{GROUP_NOTE[group]}</p>
        ) : null}
        <dl className="flex flex-col">
          {SHORTCUTS.filter((s) => s.group === group).map((s) => (
            <div
              className="flex items-center justify-between gap-4 border-border border-b py-1.5 text-sm last:border-b-0"
              data-shortcut={s.id}
              key={s.id}
            >
              <dt>{s.label}</dt>
              <dd className="flex shrink-0 items-center gap-1">
                <ShortcutKeys id={s.id} mac={mac} />
              </dd>
            </div>
          ))}
        </dl>
      </section>
    ))}
  </div>
);

/** The dialog, opened by `?` from anywhere or by `ShortcutsButton`. */
export const ShortcutsDialog = ({
  onOpenChange,
  open,
}: {
  readonly onOpenChange: (open: boolean) => void;
  readonly open: boolean;
}) => {
  useShortcut("help", () => onOpenChange(true));
  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent className="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Keyboard shortcuts</DialogTitle>
          <DialogDescription>
            Single keys do nothing while you type in a text field.
          </DialogDescription>
        </DialogHeader>
        <ShortcutsList mac={isMacPlatform()} />
      </DialogContent>
    </Dialog>
  );
};

/** Footer button that opens the dialog; icon-only on the collapsed rail. */
export const ShortcutsButton = ({
  iconOnly = false,
  onOpen,
}: {
  readonly iconOnly?: boolean;
  readonly onOpen: () => void;
}) => (
  <Tooltip>
    <TooltipTrigger asChild>
      <Button
        aria-keyshortcuts={ariaKeys("help")}
        aria-label="Keyboard shortcuts"
        onClick={onOpen}
        size={iconOnly ? "icon-sm" : "sm"}
        variant="ghost"
      >
        <KeyboardIcon />
        {iconOnly ? null : <span>Shortcuts</span>}
      </Button>
    </TooltipTrigger>
    <TooltipContent className="flex items-center gap-1.5" side="right">
      Keyboard shortcuts
      <ShortcutKeys id="help" />
    </TooltipContent>
  </Tooltip>
);
