//! Running `$EDITOR`, against `tests/fixtures/echo-editor`, then through the pure
//! decision and the reducer.

mod common;

use std::path::Path;

use common::*;
use gh_board::model::OpKind;
use gh_board::shell::editor::edit;
use gh_board::ui::editor::{self, Edit, EditKind};
use gh_board::update::{Msg, update};

/// The fake editor, `tests/fixtures/echo-editor`, with its options.
fn fake_editor(options: &[&str]) -> Vec<String> {
    std::iter::once(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/echo-editor"
    ))
    .chain(options.iter().copied())
    .map(String::from)
    .collect()
}

/// Split what an `--echo` editor saved into its arguments and what it saw.
fn echoed(text: &str) -> (Vec<&str>, &str) {
    let (args, seen) = text.split_once("\n--\n").expect("an echoed file");
    (args.lines().collect(), seen)
}

#[test]
fn the_editor_gets_the_initial_text_in_a_md_file_and_its_edit_comes_back() {
    let out = edit(&fake_editor(&["--echo"]), "Old body").unwrap();
    assert!(out.success);
    let (args, seen) = echoed(&out.text);
    assert_eq!(seen, "Old body");
    let file = args.last().expect("a file argument");
    assert!(file.ends_with(".md"), "{file}");
    assert!(!Path::new(file).exists(), "the temp file is removed");
}

#[test]
fn editor_arguments_come_before_the_file() {
    let out = edit(&fake_editor(&["--echo", "--wait"]), "").unwrap();
    let (args, _) = echoed(&out.text);
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], "--wait");
}

#[test]
fn a_non_zero_exit_is_reported_and_the_file_still_removed() {
    let out = edit(&fake_editor(&["--echo", "--exit", "1"]), "").unwrap();
    assert!(!out.success);
    let (args, _) = echoed(&out.text);
    assert!(!Path::new(args.last().unwrap()).exists());
}

#[test]
fn a_missing_editor_is_an_error() {
    let cmd = vec!["/nonexistent/gh-board-editor".to_string()];
    assert!(edit(&cmd, "x").is_err());
    assert!(edit(&[], "x").is_err());
}

/// Edit `initial` with an editor that saves `write` (or leaves the file as it
/// was) and exits with `code`, then decide what that means.
fn edit_and_decide(request: &Edit, write: Option<&str>, code: i32) -> Option<Msg> {
    let code = code.to_string();
    let mut options = vec!["--exit", &code];
    if let Some(text) = write {
        options.extend(["--write", text]);
    }
    let out = edit(&fake_editor(&options), &request.initial).unwrap();
    editor::decide(request, out.success, &out.text)
}

fn round_trip(kind: EditKind, write: Option<&str>, code: i32) -> Vec<OpKind> {
    let request = Edit {
        kind,
        initial: "Draft the post.\n\nThen ask for review.".into(),
    };
    let Some(msg) = edit_and_decide(&request, write, code) else {
        return vec![];
    };
    let (s, _) = update(detail_open(), msg);
    s.ops.into_iter().map(|op| op.kind).collect()
}

#[test]
fn a_saved_comment_is_posted() {
    assert_eq!(
        round_trip(
            EditKind::Comment {
                item_id: "item3".into()
            },
            Some("Done!\n"),
            0
        ),
        vec![OpKind::Comment {
            body: "Done!".into()
        }]
    );
}

#[test]
fn an_empty_comment_or_cq_posts_nothing() {
    assert_eq!(
        round_trip(
            EditKind::Comment {
                item_id: "item3".into()
            },
            Some("  \n"),
            0
        ),
        vec![]
    );
    assert_eq!(
        round_trip(
            EditKind::Comment {
                item_id: "item3".into()
            },
            Some("Done!"),
            1
        ),
        vec![]
    );
}

#[test]
fn a_saved_description_is_set() {
    assert_eq!(
        round_trip(
            EditKind::Description {
                item_id: "item3".into()
            },
            Some("Ship it.\n"),
            0
        ),
        vec![OpKind::Body {
            body: "Ship it.".into()
        }]
    );
}

#[test]
fn an_untouched_description_changes_nothing() {
    assert_eq!(
        round_trip(
            EditKind::Description {
                item_id: "item3".into()
            },
            None,
            0
        ),
        vec![]
    );
}

#[test]
fn feedback_is_sent_with_what_was_saved_and_cq_cancels_it() {
    let request = Edit {
        kind: EditKind::Feedback {
            title: "Make it pink".into(),
        },
        initial: String::new(),
    };
    let feedback = |write, code| edit_and_decide(&request, write, code);
    assert_eq!(
        feedback(Some("Pinker.\n"), 0),
        Some(Msg::SendFeedback {
            title: "Make it pink".into(),
            body: "Pinker.".into(),
        })
    );
    assert_eq!(
        feedback(None, 0),
        Some(Msg::SendFeedback {
            title: "Make it pink".into(),
            body: String::new(),
        }),
        "saving an empty file sends the title alone"
    );
    assert_eq!(feedback(Some("Pinker."), 1), None);
}
