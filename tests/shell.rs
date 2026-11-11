//! The imperative shell, with the network replaced: `Gh` via a fake `gh`
//! program, `perform` via a scripted transport.

mod common;

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

use common::*;
use gh_board::model::{OpKind, OpState, State, view};
use gh_board::queries::Request;
use gh_board::shell::effects::{Runner, perform};
use gh_board::shell::gh::{Gh, Transport, parse};
use gh_board::types::Card;
use gh_board::update::{Effect, Level, Msg, MutationResult, update};
use serde_json::{Value, json};

const PF: &str = "f-priority";

// parse -----------------------------------------------------------------------

#[test]
fn parse_returns_data_on_success() {
    assert_eq!(
        parse(Some(0), br#"{"data":{"viewer":{"login":"me"}}}"#, b""),
        Ok(json!({ "viewer": { "login": "me" } }))
    );
}

#[test]
fn parse_joins_graphql_error_messages_even_alongside_data() {
    let out = br#"{"data":{"node":null},"errors":[{"message":"a"},{"message":"b"}]}"#;
    assert_eq!(parse(Some(1), out, b"gh: a"), Err("a; b".into()));
}

#[test]
fn parse_falls_back_to_stderr_then_the_exit_code() {
    assert_eq!(
        parse(Some(4), b"", b"  auth required \n"),
        Err("auth required".into())
    );
    assert_eq!(
        parse(Some(4), b"", b""),
        Err("gh exited with code 4".into())
    );
    assert_eq!(
        parse(None, b"", b""),
        Err("gh was killed by a signal".into())
    );
    assert_eq!(
        parse(Some(0), br#"{"data":null}"#, b""),
        Err("gh exited with code 0".into())
    );
}

// Gh --------------------------------------------------------------------------

/// A fake `gh` that records its argv and stdin, then prints `response`.
fn fake_gh(name: &str, response: &str) -> (Gh, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("gh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{d}/argv'\ncat > '{d}/stdin'\nprintf '%s' '{response}'\n",
            d = dir.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    (Gh::new(script), dir)
}

#[test]
fn gh_sends_the_request_as_json_on_stdin_never_in_argv() {
    let (gh, dir) = fake_gh("gh_stdin", r#"{"data":{"ok":true}}"#);
    let request = gh_board::queries::comment("I_1", "@kris can you look?");
    assert_eq!(gh.graphql(&request), Ok(json!({ "ok": true })));
    let argv = std::fs::read_to_string(dir.join("argv")).unwrap();
    assert_eq!(argv, "api\ngraphql\n--input\n-\n");
    let stdin: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("stdin")).unwrap()).unwrap();
    assert_eq!(stdin, serde_json::to_value(&request).unwrap());
}

#[test]
fn gh_that_cannot_be_spawned_reports_an_error() {
    let gh = Gh::new("/nonexistent/gh-board-test/gh");
    let err = gh.graphql(&gh_board::queries::detail("I")).unwrap_err();
    assert!(err.starts_with("could not run gh:"), "{err}");
}

// perform ---------------------------------------------------------------------

/// A transport that answers from a script, in order, and records requests.
struct Fake {
    answers: Mutex<VecDeque<Result<Value, String>>>,
    seen: Mutex<Vec<Request>>,
}

impl Fake {
    fn new(answers: impl IntoIterator<Item = Result<Value, String>>) -> Self {
        Fake {
            answers: Mutex::new(answers.into_iter().collect()),
            seen: Mutex::new(vec![]),
        }
    }

    fn push(&self, answer: Result<Value, String>) {
        self.answers.lock().unwrap().push_back(answer);
    }

    /// The first top-level field each request asked for, e.g. `createIssue`.
    fn calls(&self) -> Vec<String> {
        const FIELDS: [&str; 9] = [
            "organization(",
            "createIssue(",
            "addProjectV2ItemById(",
            "updateProjectV2ItemFieldValue(",
            "setIssueFieldValue(",
            "addComment(",
            "repository(",
            "node(",
            "closeIssue(",
        ];
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|r| {
                let first = FIELDS
                    .iter()
                    .filter_map(|f| r.query.find(f).map(|i| (i, f)))
                    .min();
                first.map_or("?".into(), |(_, f)| f.trim_end_matches('(').to_string())
            })
            .collect()
    }
}

impl Transport for Fake {
    fn graphql(&self, request: &Request) -> Result<Value, String> {
        self.seen.lock().unwrap().push(request.clone());
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .expect("an answer for every request")
    }
}

fn run(gh: &Fake, effect: Effect) -> Vec<Msg> {
    let mut out = vec![];
    perform(gh, &config(), effect, |_| {}, |m| out.push(m));
    out
}

/// Run messages through the reducer, performing every network effect at once
/// against `gh` and feeding the outcomes back, until everything settles.
fn drive(mut s: State, msg: Msg, gh: &Fake) -> State {
    let mut queue = VecDeque::from([msg]);
    while let Some(m) = queue.pop_front() {
        let (next, fx) = update(s, m);
        s = next;
        for e in fx {
            perform(gh, &config(), e, |_| {}, |m| queue.push_back(m));
        }
    }
    s
}

fn item_json(card: &Card) -> Value {
    gh::item(card, PF)
}

#[test]
fn the_board_is_fetched_page_by_page_and_decoded_as_one() {
    let all: Vec<Card> = items().iter().cloned().collect();
    let page =
        |cards: &[Card], cursor| gh::page(&meta(), cards.iter().map(item_json).collect(), cursor);
    let gh = Fake::new([Ok(page(&all[..2], Some("c1"))), Ok(page(&all[2..], None))]);
    let msgs = run(&gh, Effect::FetchBoard { seq: 3 });
    let [Msg::BoardLoaded { seq: 3, board }] = &msgs[..] else {
        panic!("{msgs:?}")
    };
    assert_eq!(board.cards, items());
    let seen = gh.seen.lock().unwrap();
    assert_eq!(seen[0].variables.get("after"), None);
    assert_eq!(seen[1].variables["after"], "c1");
}

#[test]
fn a_failed_or_undecodable_board_is_reported() {
    let gh = Fake::new([Err("rate limited".into())]);
    assert_eq!(
        run(&gh, Effect::FetchBoard { seq: 1 }),
        vec![Msg::BoardFailed {
            seq: 1,
            err: "rate limited".into()
        }]
    );
    let gh = Fake::new([Ok(json!({ "organization": null }))]);
    let msgs = run(&gh, Effect::FetchBoard { seq: 1 });
    assert!(
        matches!(&msgs[..], [Msg::BoardFailed { err, .. }] if err.starts_with("organization not found"))
    );
}

#[test]
fn a_mutation_is_sent_and_its_response_decoded_for_the_op() {
    let s = loaded();
    let (_, fx) = update(
        s,
        Msg::SetPriority {
            item_id: "item1".into(),
            priority_id: Some("p-low".into()),
        },
    );
    let changed = Card {
        priority_id: Some("p-low".into()),
        ..card(1)
    };
    let gh = Fake::new([Ok(
        json!({ "setIssueFieldValue": { "issue": gh::issue(&changed, PF) } }),
    )]);
    let msgs = run(&gh, fx.into_iter().next().unwrap());
    let [
        Msg::OpOk {
            result: Ok(MutationResult::Card(c)),
            ..
        },
    ] = &msgs[..]
    else {
        panic!("{msgs:?}")
    };
    assert_eq!(c.priority_id.as_deref(), Some("p-low"));
    assert_eq!(gh.calls(), vec!["setIssueFieldValue"]);
}

#[test]
fn a_failed_mutation_is_op_failed() {
    let (_, fx) = update(
        loaded(),
        Msg::Comment {
            item_id: "item1".into(),
            body: "hi".into(),
        },
    );
    let gh = Fake::new([Err("boom".into())]);
    let msgs = run(&gh, fx.into_iter().next().unwrap());
    assert!(matches!(&msgs[..], [Msg::OpFailed { err, .. }] if err == "boom"));
}

fn created(n: u64, status: Option<&str>) -> Card {
    Card {
        status_id: status.map(String::from),
        title: "New".into(),
        ..card(n)
    }
}

fn create_answers(gh: &Fake, status: Option<&str>) {
    gh.push(Ok(json!({ "createIssue": { "issue": { "id": "issue9", "number": 9, "url": "https://x/9" } } })));
    gh.push(Ok(
        json!({ "addProjectV2ItemById": { "item": item_json(&created(9, None)) } }),
    ));
    if status.is_some() {
        gh.push(Ok(
            json!({ "updateProjectV2ItemFieldValue": { "projectV2Item": item_json(&created(9, status)) } }),
        ));
    }
}

#[test]
fn creating_an_issue_creates_adds_and_sets_its_status() {
    let gh = Fake::new([]);
    create_answers(&gh, Some("s-todo"));
    let s = drive(
        loaded(),
        Msg::Create {
            title: "New".into(),
        },
        &gh,
    );
    assert_eq!(
        gh.calls(),
        vec![
            "createIssue",
            "addProjectV2ItemById",
            "updateProjectV2ItemFieldValue"
        ]
    );
    assert!(s.ops.is_empty());
    assert_eq!(view(&s).cards["item9"], created(9, Some("s-todo")));
}

#[test]
fn creating_without_a_status_stops_after_adding_it() {
    let gh = Fake::new([]);
    create_answers(&gh, None);
    // Only a No Status column, so the new issue has no status.
    let s = loaded_with(
        config(),
        cards([Card {
            status_id: None,
            ..card(1)
        }]),
    );
    let s = drive(
        s,
        Msg::Create {
            title: "New".into(),
        },
        &gh,
    );
    assert_eq!(gh.calls(), vec!["createIssue", "addProjectV2ItemById"]);
    assert_eq!(view(&s).cards["item9"], created(9, None));
}

#[test]
fn a_retried_create_resumes_rather_than_creating_twice() {
    let gh = Fake::new([
        Ok(
            json!({ "createIssue": { "issue": { "id": "issue9", "number": 9, "url": "https://x/9" } } }),
        ),
        Err("could not add to project".into()),
    ]);
    let s = drive(
        loaded(),
        Msg::Create {
            title: "New".into(),
        },
        &gh,
    );
    let op = &s.ops[0];
    assert_eq!(op.state, OpState::Failed);
    assert!(
        matches!(&op.kind, OpKind::Create { created_issue_id: Some(id), .. } if id == "issue9")
    );

    gh.seen.lock().unwrap().clear();
    gh.push(Ok(
        json!({ "addProjectV2ItemById": { "item": item_json(&created(9, None)) } }),
    ));
    gh.push(Ok(json!({ "updateProjectV2ItemFieldValue": { "projectV2Item": item_json(&created(9, Some("s-todo"))) } })));
    let op_id = op.id;
    let s = drive(s, Msg::OpRetry { op_id }, &gh);
    assert_eq!(
        gh.calls(),
        vec!["addProjectV2ItemById", "updateProjectV2ItemFieldValue"]
    );
    assert!(gh.seen.lock().unwrap()[0].variables["contentId"] == "issue9");
    assert!(s.ops.is_empty());
    assert!(view(&s).cards.contains_key("item9"));
}

// Feedback --------------------------------------------------------------------

#[test]
fn feedback_looks_up_gh_boards_repo_then_files_the_issue_there() {
    let gh = Fake::new([
        Ok(json!({ "repository": { "id": "R_gh_board" } })),
        Ok(json!({ "createIssue": { "issue": { "id": "I1", "url": "https://x/1" } } })),
    ]);
    let out = run(
        &gh,
        Effect::SendFeedback {
            title: "Make it pink".into(),
            body: "Pinker.".into(),
        },
    );
    assert_eq!(
        out,
        vec![Msg::FeedbackSent {
            url: Some("https://x/1".into())
        }]
    );
    assert_eq!(gh.calls(), vec!["repository", "createIssue"]);
    let seen = gh.seen.lock().unwrap();
    assert_eq!(seen[1].variables["repoId"], "R_gh_board");
    assert_eq!(seen[1].variables["title"], "Make it pink");
    let body = seen[1].variables["body"].as_str().unwrap();
    assert!(body.starts_with("Pinker.\n\n---\n"), "{body}");
}

#[test]
fn feedback_fails_when_the_repo_cannot_be_seen_and_files_nothing() {
    // A user without access to the repository gets `repository: null`.
    let gh = Fake::new([Ok(json!({ "repository": null }))]);
    let out = run(
        &gh,
        Effect::SendFeedback {
            title: "Make it pink".into(),
            body: "Pinker.".into(),
        },
    );
    assert!(matches!(
        out.as_slice(),
        [Msg::FeedbackFailed { title, body, .. }] if title == "Make it pink" && body == "Pinker."
    ));
    assert_eq!(gh.calls(), vec!["repository"]);
}

#[test]
fn a_delayed_reread_waits_then_reports_the_card() {
    let gh = Fake::new([Ok(json!({ "node": item_json(&card(1)) }))]);
    let slept = Mutex::new(vec![]);
    let mut msgs = vec![];
    let effect = Effect::FetchItem {
        item_id: "item1".into(),
        delay_ms: Some(4000),
        rev: 7,
        priority_field_id: Some(PF.into()),
    };
    perform(
        &gh,
        &config(),
        effect,
        |d| slept.lock().unwrap().push(d),
        |m| msgs.push(m),
    );
    assert_eq!(*slept.lock().unwrap(), vec![Duration::from_millis(4000)]);
    assert_eq!(
        msgs,
        vec![Msg::ItemLoaded {
            rev: 7,
            card: card(1)
        }]
    );
}

#[test]
fn a_failed_reread_is_silent() {
    let gh = Fake::new([Err("nope".into())]);
    let effect = Effect::FetchItem {
        item_id: "item1".into(),
        delay_ms: None,
        rev: 0,
        priority_field_id: None,
    };
    assert!(run(&gh, effect).is_empty());
}

#[test]
fn assignable_users_and_detail_are_fetched_and_decoded() {
    let gh = Fake::new([
        Ok(
            json!({ "repository": { "assignableUsers": { "nodes": [{ "id": "u1", "login": "zoe", "name": "Zoe" }] } } }),
        ),
        Ok(json!({ "node": { "body": "b", "comments": { "totalCount": 0, "nodes": [] } } })),
    ]);
    let msgs = run(
        &gh,
        Effect::FetchAssignable {
            repo_id: "r".into(),
            repo: "acme/plans".into(),
        },
    );
    assert!(
        matches!(&msgs[..], [Msg::AssignableLoaded { users, .. }] if users[0].name.as_deref() == Some("Zoe"))
    );
    assert_eq!(
        gh.seen.lock().unwrap()[0].variables,
        json!({ "owner": "acme", "name": "plans" })
    );
    let msgs = run(
        &gh,
        Effect::FetchDetail {
            seq: 2,
            issue_id: "issue1".into(),
        },
    );
    assert!(matches!(&msgs[..], [Msg::DetailLoaded { seq: 2, .. }]));
}

// Runner ----------------------------------------------------------------------

#[test]
fn the_runner_hands_ui_effects_back_and_runs_network_ones_in_the_background() {
    let (tx, rx) = mpsc::channel();
    let gh = Fake::new([Ok(json!({ "node": { "body": "b", "comments": null } }))]);
    let runner = Runner::new(gh, config(), tx);
    let notify = Effect::Notify {
        message: "hi".into(),
        level: Level::Info,
    };
    assert_eq!(runner.run(notify.clone()), Some(notify));
    assert_eq!(
        runner.run(Effect::FetchDetail {
            seq: 1,
            issue_id: "I".into()
        }),
        None
    );
    let msg = rx.recv_timeout(Duration::from_secs(5)).expect("an answer");
    assert!(matches!(msg, Msg::DetailLoaded { seq: 1, .. }));
}
