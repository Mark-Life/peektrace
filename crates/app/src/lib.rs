//! The wasm entry of the peektrace app.

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen(start))]
pub fn start() {
    rune::run(peektrace_ui::spec());
}
