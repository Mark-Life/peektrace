//! Writes the synthetic test HOME to `$TUI_MEDIA_HOME` for screenshots and
//! tapes (`scripts/rust/tui-media/run.sh`). The HOME must sit under the
//! repo's `.scratch` dir.

mod common;

use std::path::PathBuf;

#[test]
#[ignore = "writes a HOME for media; run on demand"]
fn write_media_home() {
    let Some(dest) = std::env::var_os("TUI_MEDIA_HOME").map(PathBuf::from) else {
        panic!("set TUI_MEDIA_HOME");
    };
    let scratch = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.scratch");
    let scratch = scratch.canonicalize().expect("a .scratch dir");
    std::fs::create_dir_all(&dest).expect("mkdir");
    let dest = dest.canonicalize().expect("dest");
    assert!(
        dest.starts_with(&scratch),
        "{} is not under .scratch",
        dest.display()
    );
    let built = common::build_home();
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::rename(&built, &dest).expect("move HOME");
}
