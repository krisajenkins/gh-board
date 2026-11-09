//! The shell. For now, a placeholder that proves the terminal plumbing works.

use ratatui::crossterm::event::{self, Event, KeyCode};
use ratatui::widgets::{Block, BorderType, Paragraph};

fn main() -> std::io::Result<()> {
    ratatui::run(|terminal| {
        loop {
            terminal.draw(|frame| {
                let block = Block::bordered()
                    .border_type(BorderType::Rounded)
                    .title(" gh-board ");
                frame.render_widget(
                    Paragraph::new("Nothing here yet. q quits.").block(block),
                    frame.area(),
                );
            })?;
            if let Event::Key(key) = event::read()?
                && key.is_press()
                && key.code == KeyCode::Char('q')
            {
                break Ok(());
            }
        }
    })
}
