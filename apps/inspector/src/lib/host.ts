/** What the inspector knows about the window hosting it.
 *
 * The desktop shell serves the same bundle as the browser build, so the page
 * only learns about native window chrome from the URL it was loaded with: the
 * shell appends `?titlebar=inset` when its title bar overlays the web content
 * (macOS `hiddenInset`), where the traffic lights would otherwise sit on top
 * of the UI. In a browser the flag is absent and no space is reserved.
 */

/** True when native window controls overlay the top-left of the page. */
export const hasInsetTitlebar = (): boolean =>
  new URLSearchParams(window.location.search).get("titlebar") === "inset";
