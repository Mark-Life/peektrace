const TRAILING_SEPARATORS = /[\\/]+$/;

const isSeparator = (char: string | undefined) => char === "/" || char === "\\";

/** Show `path` with the user's home dir collapsed to `~` (display only).
 * Accepts `/` and `\` separators, so Windows paths collapse too. */
export const tildePath = ({
  path,
  home,
}: {
  readonly path: string;
  readonly home: string;
}) => {
  const base = home.replace(TRAILING_SEPARATORS, "");
  if (base === "") {
    return path;
  }
  if (path === base) {
    return "~";
  }
  return path.startsWith(base) && isSeparator(path[base.length])
    ? `~${path.slice(base.length)}`
    : path;
};
