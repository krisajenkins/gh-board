//! Editing a comment, a description or a feedback description in the user's
//! `$EDITOR`: which command to run, and what the result means. Pure;
//! `shell::editor` runs it.

use crate::types::ItemId;
use crate::update::Msg;

/// What the text is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditKind {
    Comment {
        item_id: ItemId,
    },
    Description {
        item_id: ItemId,
    },
    /// The description of feedback whose title has already been typed.
    Feedback {
        title: String,
    },
}

/// A request to edit text, for the shell to carry out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub kind: EditKind,
    /// What the file starts with.
    pub initial: String,
}

/// The editor command: `$VISUAL`, then `$EDITOR`, then `vi`, split on
/// whitespace so `code --wait` works. The file name is appended when run.
pub fn command(visual: Option<&str>, editor: Option<&str>) -> Vec<String> {
    [visual, editor]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("vi")
        .split_whitespace()
        .map(String::from)
        .collect()
}

/// What an editor session means: the message to send, if any. A non-zero
/// exit (`:cq`) discards the text (and, for feedback, cancels it); so does an
/// empty comment. Empty feedback is sent with its title alone. An unchanged
/// description is left to the reducer, which says so.
pub fn decide(edit: &Edit, success: bool, text: &str) -> Option<Msg> {
    if !success {
        return None;
    }
    match &edit.kind {
        EditKind::Comment { .. } if text.trim().is_empty() => None,
        EditKind::Comment { item_id } => Some(Msg::Comment {
            item_id: item_id.clone(),
            body: text.into(),
        }),
        EditKind::Description { item_id } => Some(Msg::SetBody {
            item_id: item_id.clone(),
            body: text.into(),
        }),
        EditKind::Feedback { title } => Some(Msg::SendFeedback {
            title: title.clone(),
            body: text.trim().into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(kind: EditKind) -> Edit {
        Edit {
            kind,
            initial: String::new(),
        }
    }

    fn comment() -> Edit {
        edit(EditKind::Comment {
            item_id: "item1".into(),
        })
    }

    fn description() -> Edit {
        edit(EditKind::Description {
            item_id: "item1".into(),
        })
    }

    fn feedback() -> Edit {
        edit(EditKind::Feedback {
            title: "Make it pink".into(),
        })
    }

    #[test]
    fn visual_then_editor_then_vi() {
        assert_eq!(command(Some("nvim"), Some("nano")), ["nvim"]);
        assert_eq!(command(None, Some("nano")), ["nano"]);
        assert_eq!(command(Some("  "), Some("nano")), ["nano"]);
        assert_eq!(command(None, None), ["vi"]);
        assert_eq!(command(Some(""), Some("")), ["vi"]);
    }

    #[test]
    fn editor_arguments_are_kept() {
        assert_eq!(command(None, Some("code --wait")), ["code", "--wait"]);
    }

    #[test]
    fn a_comment_is_posted_when_saved_with_text() {
        assert_eq!(
            decide(&comment(), true, "Looks good\n"),
            Some(Msg::Comment {
                item_id: "item1".into(),
                body: "Looks good\n".into(),
            })
        );
    }

    #[test]
    fn an_empty_comment_is_dropped_quietly() {
        assert_eq!(decide(&comment(), true, "  \n\n"), None);
    }

    #[test]
    fn a_failed_editor_discards_everything() {
        assert_eq!(decide(&comment(), false, "text"), None);
        assert_eq!(decide(&description(), false, "text"), None);
        assert_eq!(decide(&feedback(), false, "text"), None);
    }

    #[test]
    fn a_description_is_set_even_when_empty() {
        assert_eq!(
            decide(&description(), true, ""),
            Some(Msg::SetBody {
                item_id: "item1".into(),
                body: String::new(),
            })
        );
    }

    #[test]
    fn feedback_is_sent_with_its_description_trimmed_or_with_none() {
        assert_eq!(
            decide(&feedback(), true, "\nIt should be pinker.\n\n"),
            Some(Msg::SendFeedback {
                title: "Make it pink".into(),
                body: "It should be pinker.".into(),
            })
        );
        assert_eq!(
            decide(&feedback(), true, "  \n"),
            Some(Msg::SendFeedback {
                title: "Make it pink".into(),
                body: String::new(),
            })
        );
    }
}
