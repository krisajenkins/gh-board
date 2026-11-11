//! Running `$EDITOR`, against fake editor scripts, then through the pure
//! decision and the reducer.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::*;
use gh_board::model::OpKind;
use gh_board::shell::editor::{Edited, edit};
use gh_board::ui::editor::{self, Edit, EditKind};
use gh_board::update::{Msg, update};

/// A fake editor: records its argv and what the file held, replaces the file
/// with `write` (if given), and exits with `code`.
fn fake_editor(name: &str, write: Option<&str>, code: i32) -> (Vec<String>, PathBuf) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("editor");
    let replace = write
        .map(|w| format!("printf '%s' '{w}' > \"$f\"\n"))
        .unwrap_or_default();
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nfor f; do :; done\nprintf '%s\\n' \"$@\" > '{d}/argv'\ncp \"$f\" '{d}/seen'\n{replace}exit {code}\n",
            d = dir.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    (vec![script.display().to_string()], dir)
}

fn argv(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("argv"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect()
}

#[test]
fn the_editor_gets_the_initial_text_in_a_md_file_and_its_edit_comes_back() {
    let (cmd, dir) = fake_editor("ed_roundtrip", Some("New body"), 0);
    let out = edit(&cmd, "Old body").unwrap();
    assert_eq!(
        out,
        Edited {
            success: true,
            text: "New body".into(),
        }
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("seen")).unwrap(),
        "Old body"
    );
    let args = argv(&dir);
    let file = args.last().expect("a file argument");
    assert!(file.ends_with(".md"), "{file}");
    assert!(!Path::new(file).exists(), "the temp file is removed");
}

#[test]
fn editor_arguments_come_before_the_file() {
    let (mut cmd, dir) = fake_editor("ed_args", None, 0);
    cmd.push("--wait".into());
    edit(&cmd, "").unwrap();
    let args = argv(&dir);
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], "--wait");
}

#[test]
fn a_non_zero_exit_is_reported_and_the_file_still_removed() {
    let (cmd, dir) = fake_editor("ed_cq", Some("half written"), 1);
    let out = edit(&cmd, "").unwrap();
    assert!(!out.success);
    let file = argv(&dir).pop().unwrap();
    assert!(!Path::new(&file).exists());
}

#[test]
fn a_missing_editor_is_an_error() {
    let cmd = vec!["/nonexistent/gh-board-editor".to_string()];
    assert!(edit(&cmd, "x").is_err());
    assert!(edit(&[], "x").is_err());
}

fn round_trip(name: &str, kind: EditKind, write: Option<&str>, code: i32) -> Vec<OpKind> {
    let s = detail_open();
    let request = Edit {
        kind,
        initial: "Draft the post.\n\nThen ask for review.".into(),
    };
    let (cmd, _) = fake_editor(name, write, code);
    let out = edit(&cmd, &request.initial).unwrap();
    let Some(msg) = editor::decide(&request, out.success, &out.text) else {
        return vec![];
    };
    let (s, _) = update(s, msg);
    s.ops.into_iter().map(|op| op.kind).collect()
}

#[test]
fn a_saved_comment_is_posted() {
    assert_eq!(
        round_trip(
            "rt_comment",
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
            "rt_empty",
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
            "rt_cq",
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
            "rt_body",
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
            "rt_same",
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
    let feedback = |name, write, code| {
        let request = Edit {
            kind: EditKind::Feedback {
                title: "Make it pink".into(),
            },
            initial: String::new(),
        };
        let (cmd, _) = fake_editor(name, write, code);
        let out = edit(&cmd, &request.initial).unwrap();
        editor::decide(&request, out.success, &out.text)
    };
    assert_eq!(
        feedback("rt_feedback", Some("Pinker.\n"), 0),
        Some(Msg::SendFeedback {
            title: "Make it pink".into(),
            body: "Pinker.".into(),
        })
    );
    assert_eq!(
        feedback("rt_feedback_blank", None, 0),
        Some(Msg::SendFeedback {
            title: "Make it pink".into(),
            body: String::new(),
        }),
        "saving an empty file sends the title alone"
    );
    assert_eq!(feedback("rt_feedback_cq", Some("Pinker."), 1), None);
}
