//! gh-board: parse the options, then hand the terminal to the app loop.

use clap::Parser;

use gh_board::cli::Cli;
use gh_board::shell::app;

fn main() -> std::io::Result<()> {
    let config = Cli::parse().into();
    ratatui::run(|terminal| app::run(terminal, config))
}
