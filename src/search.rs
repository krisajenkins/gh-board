//! Fuzzy card search, fzf-flavoured.
//!
//! - The query splits on whitespace into terms; a card matches when every
//!   term matches.
//! - A term matches a field when its characters appear in that field in
//!   order, not necessarily adjacent: `rlnts` matches "Release notes".
//! - Each term must match within a single field, so `alice blog` can be
//!   satisfied by an assignee and a label, but one term never strings
//!   characters together across two fields.
//! - A term starting with `'` matches exactly: `'idc` matches "idc" but not
//!   "in dev channel".
//! - Smart case: a lower-case term ignores case; a term with any capital in it
//!   is case-sensitive.
//!
//! The fields are what a card shows: `#<number>`, the title, `@<login>` for
//! each assignee, each label, and the priority name.

use crate::types::Card;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Term {
    /// Folded to lower case unless `exact`.
    chars: Vec<char>,
    /// Case-sensitive: the term has a capital in it.
    exact: bool,
    /// Set for a `'term`: match this contiguously instead.
    substring: Option<String>,
}

/// Empty when the query is blank.
pub fn parse(query: &str) -> Vec<Term> {
    query
        .split_whitespace()
        .map(|word| {
            let quoted = word.len() > 1 && word.starts_with('\'');
            let word = if quoted { &word[1..] } else { word };
            let exact = word.chars().any(char::is_uppercase);
            let folded = if exact {
                word.to_string()
            } else {
                word.to_lowercase()
            };
            Term {
                chars: folded.chars().collect(),
                exact,
                substring: quoted.then_some(folded),
            }
        })
        .collect()
}

/// Do the term's characters occur in `s`, in order (or, for a `'term`, together)?
pub fn fuzzy(term: &Term, s: &str) -> bool {
    let folded;
    let s = if term.exact {
        s
    } else {
        folded = s.to_lowercase();
        &folded
    };
    if let Some(sub) = &term.substring {
        return s.contains(sub.as_str());
    }
    let mut rest = s.chars();
    term.chars.iter().all(|c| rest.any(|x| x == *c))
}

/// The searchable text of a card, one string per field.
pub fn fields(card: &Card, priority_name: Option<&str>) -> Vec<String> {
    let mut out = vec![card.title.clone()];
    out.extend(card.number.map(|n| format!("#{n}")));
    out.extend(card.assignees.iter().map(|u| format!("@{}", u.login)));
    out.extend(card.labels.iter().map(|l| l.name.clone()));
    out.extend(priority_name.map(str::to_string));
    out
}

pub fn matches(terms: &[Term], fields: &[String]) -> bool {
    terms.iter().all(|t| fields.iter().any(|f| fuzzy(t, f)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(query: &str, field: &str) -> bool {
        matches(&parse(query), &[field.to_string()])
    }

    #[test]
    fn subsequence() {
        assert!(hit("rlnts", "Release notes"));
        assert!(!hit("rlnts", "Release"));
    }

    #[test]
    fn smart_case() {
        assert!(hit("release", "Release notes"));
        assert!(hit("Release", "Release notes"));
        assert!(!hit("RELEASE", "Release notes"));
    }

    #[test]
    fn quoted_terms_are_contiguous() {
        assert!(hit("'idc", "idc"));
        assert!(!hit("'idc", "in dev channel"));
        assert!(hit("idc", "in dev channel"));
    }

    #[test]
    fn a_lone_quote_is_a_plain_term() {
        assert!(hit("'", "don't"));
        assert!(!hit("'", "dont"));
    }

    #[test]
    fn each_term_matches_within_one_field() {
        let fields = vec!["ab".to_string(), "cd".to_string()];
        assert!(matches(&parse("ab cd"), &fields));
        assert!(!matches(&parse("bc"), &fields));
    }

    #[test]
    fn blank_query_matches_everything() {
        assert!(parse("   ").is_empty());
        assert!(matches(&[], &[]));
    }
}
