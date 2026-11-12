//! The peektrace app: a native window over core, or a web page over the
//! local server's `/api`.

use std::sync::Arc;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    let source: Arc<dyn peektrace_ui::source::DataSource> =
        Arc::new(peektrace_ui::source::http::Http);
    peektrace_ui::app(&source).run();
}

/// Run the native window (or a headless render) over core in this process.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_native() {
    let source: Arc<dyn peektrace_ui::source::DataSource> =
        Arc::new(peektrace_ui::source::in_process::InProcess::from_env());
    peektrace_ui::app(&source).run();
}
