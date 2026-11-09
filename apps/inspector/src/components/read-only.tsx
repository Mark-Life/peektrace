/** Read-only server notices: why writes are off, and the header badge. */
import { Result, useAtomValue } from "@effect-atom/atom-react";
import { Badge } from "@workspace/ui/components/badge";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@workspace/ui/components/popover";
import { LockIcon } from "lucide-react";
import { serverInfoAtom } from "../lib/atoms";

/** The reason under a "Read-only server" title, flags marked up as code. */
export const ReadOnlyReason = () => (
  <>
    The server was started with <code>--read-only</code>, or on a non-loopback{" "}
    <code>--host</code> without <code>--allow-write</code>.
  </>
);

/** Header badge, shown only once the server reports read-only mode. A button,
 * so touch and keyboard users can open the reason too. */
export const ReadOnlyBadge = () => {
  const info = useAtomValue(serverInfoAtom);
  if (!(Result.isSuccess(info) && info.value.readOnly)) {
    return null;
  }
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Badge
          asChild
          className="h-6 min-w-6 shrink-0 cursor-pointer sm:h-5"
          variant="outline"
        >
          <button data-testid="read-only-badge" type="button">
            <LockIcon aria-hidden />
            <span className="sr-only sm:not-sr-only">Read-only</span>
          </button>
        </Badge>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        className="gap-1"
        data-testid="read-only-reason"
      >
        <p className="font-medium">Read-only server</p>
        <p className="text-muted-foreground">
          <ReadOnlyReason />
        </p>
      </PopoverContent>
    </Popover>
  );
};
