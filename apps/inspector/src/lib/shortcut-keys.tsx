/** A shortcut's keycaps, read from the shortcut table. */
import { Kbd } from "@workspace/ui/components/kbd";
import {
  isMacPlatform,
  keyLabel,
  type ShortcutId,
  shortcutOf,
} from "./shortcuts";

export const ShortcutKeys = ({
  id,
  mac = isMacPlatform(),
}: {
  readonly id: ShortcutId;
  readonly mac?: boolean;
}) =>
  shortcutOf(id).keys.map((key) => (
    <Kbd key={key}>{keyLabel({ key, mac })}</Kbd>
  ));
