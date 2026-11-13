//! The terminal session: raw mode, alternate screen, mouse and paste on
//! entry; all of it undone on exit and on a panic of the drawing thread.

use std::io::{self, Stdout, Write as _};
use std::sync::{Mutex, Once, PoisonError};
use std::thread::{self, ThreadId};

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

/// The thread that entered the terminal, while it is entered.
static OWNER: Mutex<Option<ThreadId>> = Mutex::new(None);
/// Panic reports from other threads, printed once the screen is restored.
static HELD: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn owner() -> Option<ThreadId> {
    *OWNER.lock().unwrap_or_else(PoisonError::into_inner)
}

fn set_owner(t: Option<ThreadId>) {
    *OWNER.lock().unwrap_or_else(PoisonError::into_inner) = t;
}

/// Leave the alternate screen and raw mode, then print panic reports held
/// meanwhile. Safe to call twice and from any thread.
pub fn restore() {
    set_owner(None);
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
    let held = std::mem::take(&mut *HELD.lock().unwrap_or_else(PoisonError::into_inner));
    let mut err = io::stderr();
    for line in held {
        let _ = writeln!(err, "{line}");
    }
}

static HOOK: Once = Once::new();

/// A panic on the drawing thread restores the terminal before its message
/// prints. One on another thread (a backend job, the sibling server) leaves
/// the screen alone and holds its message until the terminal is restored.
fn install_panic_hook() {
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let me = thread::current();
            match owner() {
                Some(t) if t != me.id() => {
                    let name = me.name().unwrap_or("<unnamed>");
                    HELD.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(format!("thread '{name}' {info}"));
                }
                _ => {
                    restore();
                    previous(info);
                }
            }
        }));
    });
}

pub fn enter() -> io::Result<Term> {
    install_panic_hook();
    set_owner(Some(thread::current().id()));
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
