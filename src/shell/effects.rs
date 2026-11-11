//! The effect interpreter: the reducer's network effects -> requests ->
//! decoded messages.
//!
//! [`perform`] does one effect's work, blocking, and reports every outcome as
//! a [`Msg`]. [`Runner`] runs it on a background thread and sends the messages
//! down a channel for the app loop to feed back into `update`. Effects that
//! belong to the UI (notify, prompt, save text) are not network work: `Runner`
//! hands them back.

use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use serde_json::Value;

use crate::decode;
use crate::model::{Op, OpKind};
use crate::queries;
use crate::shell::gh::Transport;
use crate::types::{Config, Meta};
use crate::update::{Effect, Msg};

/// Whether an effect is network work for [`perform`], rather than UI work.
pub fn is_network(effect: &Effect) -> bool {
    matches!(
        effect,
        Effect::FetchBoard { .. }
            | Effect::FetchAssignable { .. }
            | Effect::FetchItem { .. }
            | Effect::FetchDetail { .. }
            | Effect::Mutate { .. }
            | Effect::SendFeedback { .. }
    )
}

/// Runs network effects on background threads.
pub struct Runner<T> {
    transport: Arc<T>,
    config: Arc<Config>,
    tx: Sender<Msg>,
}

impl<T: Transport> Runner<T> {
    pub fn new(transport: T, config: Config, tx: Sender<Msg>) -> Self {
        Runner {
            transport: Arc::new(transport),
            config: Arc::new(config),
            tx,
        }
    }

    /// Start a network effect in the background. Any other effect is handed
    /// back for the UI to deal with.
    pub fn run(&self, effect: Effect) -> Option<Effect> {
        if !is_network(&effect) {
            return Some(effect);
        }
        let (transport, config, tx) =
            (self.transport.clone(), self.config.clone(), self.tx.clone());
        thread::spawn(move || {
            // A send fails only once the app has gone: late answers have
            // nowhere to go.
            perform(&*transport, &config, effect, thread::sleep, |msg| {
                let _ = tx.send(msg);
            });
        });
        None
    }
}

/// Do one network effect's work, blocking, reporting each outcome through
/// `dispatch`. `sleep` serves delayed re-reads. UI effects are ignored.
pub fn perform(
    gh: &impl Transport,
    config: &Config,
    effect: Effect,
    sleep: impl Fn(Duration),
    mut dispatch: impl FnMut(Msg),
) {
    match effect {
        Effect::FetchBoard { seq } => dispatch(match fetch_board(gh, config) {
            Ok(board) => Msg::BoardLoaded { seq, board },
            Err(err) => Msg::BoardFailed { seq, err },
        }),
        Effect::FetchAssignable { repo_id, repo } => {
            let users = gh
                .graphql(&queries::assignable(&repo))
                .and_then(|data| decode::assignable(&data));
            dispatch(match users {
                Ok(users) => Msg::AssignableLoaded { repo_id, users },
                Err(err) => Msg::AssignableFailed { repo_id, err },
            })
        }
        Effect::FetchDetail { seq, issue_id } => {
            let data = gh
                .graphql(&queries::detail(&issue_id))
                .and_then(|data| decode::detail(&data));
            dispatch(match data {
                Ok(data) => Msg::DetailLoaded { seq, data },
                Err(err) => Msg::DetailFailed { seq, err },
            })
        }
        Effect::FetchItem {
            item_id,
            delay_ms,
            rev,
            priority_field_id,
        } => {
            if let Some(ms) = delay_ms.filter(|&ms| ms > 0) {
                sleep(Duration::from_millis(ms));
            }
            // Failures are ignored: this only refines what is shown, and a
            // refresh recovers anything it misses.
            let card = gh
                .graphql(&queries::item(&item_id, &config.status_field))
                .and_then(|data| decode::item_reread(&data, priority_field_id.as_deref()));
            if let Ok(Some(card)) = card {
                dispatch(Msg::ItemLoaded { rev, card });
            }
        }
        Effect::Mutate { op, meta } => mutate(gh, config, op, &meta, &mut dispatch),
        Effect::SendFeedback { title, body } => dispatch(match send_feedback(gh, &title, &body) {
            Ok(url) => Msg::FeedbackSent { url },
            Err(err) => Msg::FeedbackFailed { title, body, err },
        }),
        Effect::Notify { .. } | Effect::PromptFailure { .. } | Effect::SaveText { .. } => {}
    }
}

/// Look up gh-board's repository, then file the issue there. Returns its url.
fn send_feedback(gh: &impl Transport, title: &str, body: &str) -> Result<Option<String>, String> {
    let (owner, name) = queries::FEEDBACK_REPO;
    let repo_id = decode::repository_id(&gh.graphql(&queries::repository_id(owner, name))?)?;
    let (_, url) = decode::created_issue(&gh.graphql(&queries::feedback(&repo_id, title, body))?)?;
    Ok(url)
}

/// Every page of the board, decoded as one.
fn fetch_board(gh: &impl Transport, config: &Config) -> Result<crate::types::Board, String> {
    let mut pages: Vec<Value> = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let data = gh.graphql(&queries::board(config, after.as_deref()))?;
        after = queries::next_cursor(&data);
        pages.push(data);
        if after.is_none() {
            break;
        }
    }
    decode::board(&pages, config).map(|(board, _skipped)| board)
}

fn mutate(
    gh: &impl Transport,
    config: &Config,
    op: Op,
    meta: &Meta,
    dispatch: &mut impl FnMut(Msg),
) {
    let op_id = op.id;
    let result = if matches!(op.kind, OpKind::Create { .. }) {
        create(gh, config, &op, meta, dispatch)
    } else {
        queries::for_op(&op, meta, config).and_then(|request| gh.graphql(&request))
    };
    let pf_id = meta.priority_field.as_ref().map(|f| f.id.as_str());
    dispatch(match result {
        Ok(data) => Msg::OpOk {
            op_id,
            result: decode::mutation(&op.kind, &data, pf_id),
        },
        Err(err) => Msg::OpFailed { op_id, err },
    });
}

/// Create runs as up to three requests: create the issue, add it to the
/// project, set its status. Once the issue exists its id is reported back
/// (`OpProgress`), so a retry after a later step fails resumes rather than
/// creating a duplicate. Returns the last step's response.
fn create(
    gh: &impl Transport,
    config: &Config,
    op: &Op,
    meta: &Meta,
    dispatch: &mut impl FnMut(Msg),
) -> Result<Value, String> {
    let OpKind::Create {
        title,
        repo_id,
        status_id,
        priority_id,
        created_issue_id,
        ..
    } = &op.kind
    else {
        unreachable!("create() is only called for a create");
    };
    let pf = meta.priority_field.as_ref();

    let issue_id = match created_issue_id {
        Some(id) => id.clone(),
        None => {
            let priority = pf.map(|f| f.id.as_str()).zip(priority_id.as_deref());
            let data = gh.graphql(&queries::create_issue(repo_id, title, priority))?;
            let (id, url) = decode::created_issue(&data)?;
            dispatch(Msg::OpProgress {
                op_id: op.id,
                created_issue_id: id.clone(),
                created_issue_url: url,
            });
            id
        }
    };

    let added = gh.graphql(&queries::add_to_project(
        &meta.project_id,
        &issue_id,
        &config.status_field,
    ))?;
    let Some(status_id) = status_id else {
        return Ok(added);
    };
    let item = decode::added_item(&added, pf.map(|f| f.id.as_str()))?;
    gh.graphql(&queries::set_status(
        &meta.project_id,
        &item.item_id,
        &meta.status_field.id,
        status_id,
        &config.status_field,
    ))
}
