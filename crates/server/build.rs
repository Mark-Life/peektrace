//! Release builds embed `apps/inspector/dist`. rust-embed tracks only the
//! files it embedded, so watch the folder too: a UI built after the first
//! release build then triggers a rebuild instead of being missed.

use std::path::Path;

const DIST: &str = "../../apps/inspector/dist";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("PROFILE").as_deref() != Ok("release") {
        return;
    }
    println!("cargo:rerun-if-changed={DIST}");
    if !Path::new(DIST).join("index.html").is_file() {
        println!(
            "cargo:warning=apps/inspector/dist/index.html is missing, so this release build has no web UI. Run `cd apps/inspector && bun run build`, then build again."
        );
    }
}
