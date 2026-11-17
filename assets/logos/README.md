# Logos

`providers/<id>.svg`: model maker logos from [models.dev](https://models.dev)
(`https://models.dev/logos/<id>.svg`), MIT license
(<https://github.com/sst/models.dev>). Each draws in `currentColor`, so the
app tints it for light and dark themes.

The ids match `peektrace_core::model_provider::ProviderMark::id`. The web
inspector and the Rune app both read these files; keep one copy. To add a
maker:

1. Download its SVG here as `providers/<id>.svg`.
2. Add the id to `MAKERS` in `crates/core/src/model_provider/build.rs` and
   run `peektrace pricing update` to refresh the snapshot.
3. Web: add it to `PROVIDER_LOGO` in `packages/viz/src/lib/provider-logos.ts`.
4. Rune: add it to `LOGOS` in `crates/ui/src/views/marks.rs`, with the SVG's
   `viewBox` size.
5. TUI: add a short mark and colour to `provider_glyph` in
   `crates/tui/src/theme.rs`.

Tests fail when a `MAKERS` id misses step 3, 4 or 5.
