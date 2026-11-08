# CLI binary size

`peektrace` is a `bun build --compile` binary for five targets (macOS arm64 and
x64, Linux arm64 and x64, Windows x64). Two build plugins in
`apps/cli/src/build-plugins.ts` make the binaries smaller (the tree-sitter stub on all five targets, the strip on Linux).

## Result

linux-x64, built with Bun 1.3.14 (the version CI pins), same commit, plugins off
and on:

| | Raw | `gzip -9` |
|---|---:|---:|
| Before | 116,242,560 B | 41,213,794 B |
| After | 102,885,504 B | 38,057,162 B |
| Change | -13.4 MB (-11.5%) | -3.2 MB (-7.7%) |

The v0.11.0 release asset is 116,058,240 B raw and 41,040,113 B gzipped, which
matches the "before" row. Debug info compresses well, so the gzip saving is
smaller than the raw one.

linux-arm64 was not built here: its OpenTUI package is not installed on the dev
machine. The stripped `libopentui.so` was checked on its own: 13,995,736 B to
3,857,544 B, no `.debug_*` sections left, same 1,929 dynamic symbols. The arm64
binary was not run.

## What is in the binary

The Bun runtime is most of the file and cannot be changed from this repo. On
top of it:

- the bundled JS of the CLI,
- the built inspector UI (see [`bundle-size.md`](bundle-size.md)),
- OpenTUI's native library (`libopentui.so`, `.dylib` or `.dll`),
- OpenTUI's parser worker. Its tree-sitter grammars and queries (`.wasm`, `.scm`) are stubbed out.

## Plugins

**`stripNativeLib`** (Linux targets only). The `@opentui/core-linux-*` libraries
ship with DWARF debug info: 13.7 MB, of which about 9.8 MB is debug data. The
plugin strips a copy with `--strip-debug` and embeds that. It keeps `.dynsym`,
which `dlopen` needs. The copy goes to `apps/cli/dist/.native/<target>/`. The original
in `node_modules` is never changed, because the package manager hardlinks it
across installs.

The macOS and Windows libraries are about 3.8 MB and carry no debug info, so
they are left alone. Stripping a `.dylib` would also break its ad-hoc signature.

**`stubTreeSitterAssets`** (all targets). OpenTUI imports tree-sitter grammars
lazily for its `<code>` and `<markdown>` renderables. peektrace does not use
them: `apps/cli/src/tui/syntax.tsx` highlights with `@workspace/core/highlight`.
The plugin replaces `.wasm` and `.scm` imports from `@opentui/*` with a module
that throws a clear error, about 3.6 MB less. The parser worker stays real
because OpenTUI loads it at startup. Remove the plugin before adopting `<code>`
or `<markdown>`.

## Strip tool

Order of lookup: `PEEKTRACE_STRIP`, `llvm-strip` on `PATH`, Homebrew `llvm`, then
GNU `strip` if the host is Linux with the target's arch. GNU `strip` cannot read
a foreign-arch ELF, and Apple `strip` cannot read ELF at all. Releases are built
on a macOS runner, so `publish-cli.yml` installs Homebrew `llvm` and sets
`PEEKTRACE_STRIP_REQUIRED=1`.

With no tool the build warns and embeds the unstripped library.
`PEEKTRACE_STRIP_REQUIRED=1` makes it fail instead.

## Upgrade patches

The first release with this change replaces the embedded library bytes. The
binary patch from v0.11.0 is larger than usual. If it passes the size limit in
the publish workflow, the workflow drops it and clients download the full file.

## Reproduce

```sh
cd apps/cli
BUN_TARGET=bun-linux-x64 bun run src/build.ts
wc -c < dist/bun-linux-x64/peektrace
gzip -9 -c dist/bun-linux-x64/peektrace | wc -c
```

For the "before" row, build from a checkout without `build-plugins.ts`. Use the
same Bun version for both.

## Not done

- Ask OpenTUI to publish stripped Linux libraries. This would make the plugin
  unnecessary.
- Homebrew tap, desktop build and AppImage sizes, and the AOT/trim options in
  PR #63.
