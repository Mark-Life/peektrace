# Logos

`providers/<id>.svg`: model maker logos from [models.dev](https://models.dev)
(`https://models.dev/logos/<id>.svg`), MIT license
(<https://github.com/sst/models.dev>). Each draws in `currentColor`, so the
app tints it for light and dark themes.

The ids match `peektrace_core::model_provider::ProviderMark::id`. To add one,
download its SVG here and add the maker to `MAKERS` in
`crates/core/src/model_provider/build.rs`. The web inspector and the Rune app
both read these files; keep one copy.
