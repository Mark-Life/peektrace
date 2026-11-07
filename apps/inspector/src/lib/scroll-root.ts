/** The element the page scrolls in.
 *
 * The shell scrolls its content pane, not the window, so a virtualized list
 * deep inside a route needs that element to know what is on screen. The shell
 * owns it and hands it down; nothing walks the DOM looking for it.
 */
import { createContext, useContext } from "react";

export const ScrollRootContext = createContext<HTMLElement | null>(null);

/** The shell's scrolling element, or null before it mounts (and in SSR). */
export const useScrollRoot = () => useContext(ScrollRootContext);
