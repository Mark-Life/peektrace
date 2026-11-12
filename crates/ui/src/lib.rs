//! peektrace views on Rune.

use rune::{AppSpec, Cx, El, text};

pub struct Ui;

fn view(_s: &Ui, _cx: &mut Cx<Ui>) -> El {
    text("peektrace")
}

pub fn spec() -> AppSpec<Ui> {
    AppSpec::new("Peektrace", (1440.0, 900.0), || Ui, view)
}
