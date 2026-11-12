//! `peektrace-app`: the desktop window. Rune's own flags (`--snap`,
//! `--tree`, `--scene`, `--size`, `--theme`) render headless; `--otel`
//! (after `--`) prints each finished span to stderr.

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let otel = rune::extra_args()
        .iter()
        .any(|a| a == "--otel" || a == "otel")
        || std::env::var_os("PEEKTRACE_OTEL").is_some_and(|v| !v.is_empty());
    if otel {
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
            .with_target(false)
            .init();
    }
    app::run_native();
}

#[cfg(target_arch = "wasm32")]
fn main() {}
