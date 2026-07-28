/** Global "a text input is focused" flag.
 *
 * When a screen's search/text field has focus, the app-level shortcuts (`q` to
 * quit, `1`–`4` to switch section) must not steal the keystroke. Screens call
 * `setTyping(true)` while their input is focused; the root keymap consults
 * `useTyping()` before acting on bare letter/number keys.
 */
import { createContext, type ReactNode, useContext, useState } from "react";

interface TypingState {
  readonly setTyping: (value: boolean) => void;
  readonly typing: boolean;
}

const TypingContext = createContext<TypingState>({
  typing: false,
  setTyping: () => {
    /* no-op default */
  },
});

/** Provide the typing flag to the tree. */
export const TypingProvider = ({
  children,
}: {
  readonly children: ReactNode;
}) => {
  const [typing, setTyping] = useState(false);
  return (
    <TypingContext value={{ typing, setTyping }}>{children}</TypingContext>
  );
};

/** Read + set whether a text input currently owns the keyboard. */
export const useTyping = (): TypingState => useContext(TypingContext);
