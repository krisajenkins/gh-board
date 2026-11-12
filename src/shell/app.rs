//! The app loop: the one place that owns the terminal.
//!
//! Draw; wait briefly for a key; drain the messages background effects have
//! sent. Every message goes through `update`; network effects go to the
//! [`Runner`], and the UI effects it hands back are applied to the UI state
//! (or, for `SaveText`, done here: the clipboard is I/O).

use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEvent};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::Rect;

use crate::model::State;
use crate::shell::editor as shell_editor;
use crate::shell::effects::Runner;
use crate::shell::gh::{Gh, Transport};
use crate::types::Config;
use crate::ui::editor;
use crate::ui::{Action, UiState, handle_key, render};
use crate::update::{Effect, Level, Msg, update};

/// How long to wait for a key before checking for messages.
const TICK: Duration = Duration::from_millis(50);

struct App {
    state: State,
    ui: UiState,
}

/// Run the board until the user quits.
pub fn run(terminal: &mut DefaultTerminal, config: Config) -> io::Result<()> {
    let (tx, rx) = mpsc::channel();
    let runner = Runner::new(Gh::default(), config.clone(), tx);
    run_with(terminal, &runner, &rx, State::new(config))
}

fn run_with<T: Transport>(
    terminal: &mut DefaultTerminal,
    runner: &Runner<T>,
    rx: &Receiver<Msg>,
    state: State,
) -> io::Result<()> {
    let mut app = App {
        state,
        ui: UiState::default(),
    };
    app = dispatch(app, runner, Msg::Refresh, Instant::now());
    let mut dirty = true;
    loop {
        if let Some(ui) = app.ui.expire(Instant::now()) {
            app.ui = ui;
            dirty = true;
        }
        if dirty {
            let size = terminal.size()?;
            app.ui = app
                .ui
                .settle(&app.state, Rect::new(0, 0, size.width, size.height));
            terminal.draw(|frame| render(frame, &app.state, &app.ui))?;
            dirty = false;
        }
        if event::poll(TICK)? {
            match event::read()? {
                Event::Key(key) if key.is_press() => {
                    dirty = true;
                    match on_key(app, runner, key, terminal)? {
                        Some(next) => app = next,
                        None => return Ok(()),
                    }
                }
                Event::Resize(..) => dirty = true,
                _ => {}
            }
        }
        while let Ok(msg) = rx.try_recv() {
            app = dispatch(app, runner, msg, Instant::now());
            dirty = true;
        }
    }
}

/// `None` when the key quits.
fn on_key<T: Transport>(
    app: App,
    runner: &Runner<T>,
    key: KeyEvent,
    terminal: &mut DefaultTerminal,
) -> io::Result<Option<App>> {
    let now = Instant::now();
    let (ui, outcome) = handle_key(&app.ui, &app.state, key);
    let mut app = outcome
        .msgs
        .into_iter()
        .fold(App { ui, ..app }, |app, msg| {
            dispatch(app, runner, msg, now)
        });
    for action in outcome.actions {
        match action {
            Action::Quit => return Ok(None),
            Action::Redraw => terminal.clear()?,
            Action::Notify(message, level) => app.ui = app.ui.notify(message, level, now),
            Action::Edit(edit) => {
                let command = editor::command(
                    std::env::var("VISUAL").ok().as_deref(),
                    std::env::var("EDITOR").ok().as_deref(),
                );
                let edited = suspended(terminal, || shell_editor::edit(&command, &edit.initial))?;
                match edited {
                    Ok(edited) => {
                        if let Some(msg) = editor::decide(&edit, edited.success, &edited.text) {
                            app = dispatch(app, runner, msg, Instant::now());
                        }
                    }
                    Err(e) => {
                        let message = format!("Could not run {}: {e}", command.join(" "));
                        app.ui = app.ui.notify(message, Level::Error, Instant::now());
                    }
                }
            }
            Action::Copy { text, what } => app.ui = copied(app.ui, &text, &what, now),
            Action::OpenUrl(url) => {
                if let Err(e) = open_url(&url) {
                    app.ui = app
                        .ui
                        .notify(format!("Could not open {url}: {e}"), Level::Error, now);
                }
            }
        }
    }
    Ok(Some(app))
}

/// Hand the terminal to `f` (an editor): leave raw mode and the alternate
/// screen, run it, then come back and repaint everything.
fn suspended<R>(terminal: &mut DefaultTerminal, f: impl FnOnce() -> R) -> io::Result<R> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    let result = f();
    execute!(io::stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    terminal.clear()?;
    Ok(result)
}

fn dispatch<T: Transport>(app: App, runner: &Runner<T>, msg: Msg, now: Instant) -> App {
    let (state, effects) = update(app.state, msg);
    let ui = effects
        .into_iter()
        .filter_map(|effect| runner.run(effect))
        .fold(app.ui, |ui, effect| ui_effect(ui, effect, now));
    App { state, ui }
}

fn ui_effect(ui: UiState, effect: Effect, now: Instant) -> UiState {
    match effect {
        Effect::SaveText { text, what } => copied(ui, &text, &what, now),
        other => ui.apply_effect(other, now),
    }
}

/// Copy `text` to the clipboard and say how it went, naming it `what`.
fn copied(ui: UiState, text: &str, what: &str, now: Instant) -> UiState {
    match copy_to_clipboard(text) {
        Ok(()) => ui.notify(format!("{what} copied to the clipboard"), Level::Info, now),
        Err(e) => ui.notify(
            format!("{what} could not be copied to the clipboard: {e}"),
            Level::Error,
            now,
        ),
    }
}

/// Open a link in the browser without waiting for it.
fn open_url(url: &str) -> io::Result<()> {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut child = Command::new(opener)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Reap it in the background so it does not linger as a zombie.
    thread::spawn(move || child.wait());
    Ok(())
}

fn clipboard_commands() -> &'static [(&'static str, &'static [&'static str])] {
    if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else {
        &[("wl-copy", &[]), ("xclip", &["-selection", "clipboard"])]
    }
}

fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let mut errors = Vec::new();
    for (program, args) in clipboard_commands() {
        match pipe_to(program, args, text) {
            Ok(()) => return Ok(()),
            Err(e) => errors.push(format!("{program}: {e}")),
        }
    }
    Err(errors.join("; "))
}

fn pipe_to(program: &str, args: &[&str], text: &str) -> Result<(), String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("exited with {status}"))
    }
}
