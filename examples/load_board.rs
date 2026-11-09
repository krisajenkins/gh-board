//! Load a real board through the whole read path: `gh` -> decode -> reducer.
//! Read-only. Prints column names and card counts, no card contents.
//!
//!     cargo run --example load_board -- <owner> <project-number>

use std::time::Instant;

use gh_board::model::{State, board};
use gh_board::shell::effects::perform;
use gh_board::shell::gh::Gh;
use gh_board::types::Config;
use gh_board::update::{Effect, Msg, update};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [owner, project] = &args[..] else {
        eprintln!("usage: load_board <owner> <project-number>");
        std::process::exit(2);
    };
    let config = Config::new(owner, project.parse().expect("a project number"));
    let started = Instant::now();
    let (mut s, mut effects) = update(State::new(config.clone()), Msg::Refresh);
    // Run only the board fetch; skip the assignable-user prefetches it triggers.
    while let Some(effect) = effects.pop() {
        if let Effect::FetchBoard { .. } = effect {
            let mut msgs = vec![];
            perform(&Gh::default(), &config, effect, |_| {}, |m| msgs.push(m));
            for m in msgs {
                let (next, fx) = update(s, m);
                s = next;
                effects.extend(
                    fx.into_iter()
                        .filter(|e| !matches!(e, Effect::FetchAssignable { .. })),
                );
            }
        } else if let Effect::Notify { message, .. } = effect {
            eprintln!("notify: {message}");
        }
    }
    if let Some(err) = &s.error {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
    let meta = s.meta.as_ref().expect("loaded");
    println!(
        "loaded {} cards in {:.1?}",
        s.base.cards.len(),
        started.elapsed()
    );
    println!(
        "priority field: {}",
        meta.priority_field
            .as_ref()
            .map_or("none".into(), |f| format!("{} options", f.options.len()))
    );
    for col in board(&s).columns {
        println!("  {:<20} {}", col.name, col.cards.len());
    }
}
