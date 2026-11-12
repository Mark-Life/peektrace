//! The terminal session: raw mode, alternate screen, mouse and paste on
//! entry; all of it undone on exit and on panic.

use std::io::{self, Stdout, Write as _};
use std::sync::Once;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::crossterm::{cursor, execute};

pub type Term = Terminal<CrosstermBackend<Stdout>>;

/// Leave the alternate screen and raw mode. Safe to call twice.
pub fn restore() {
    let mut out = io::stdout();
    let _ = execute!(
        out,
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen,
        cursor::Show
    );
    let _ = disable_raw_mode();
    let _ = out.flush();
}

static HOOK: Once = Once::new();

/// Restore the terminal before any panic message prints.
fn install_panic_hook() {
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            previous(info);
        }));
    });
}

pub fn enter() -> io::Result<Term> {
    install_panic_hook();
    enable_raw_mode()?;
    let mut out = io::stdout();
    if let Err(e) = execute!(
        out,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
        cursor::Hide
    ) {
        restore();
        return Err(e);
    }
    Terminal::new(CrosstermBackend::new(out)).inspect_err(|_| restore())
}
