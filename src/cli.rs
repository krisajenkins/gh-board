//! Command-line options -> [`Config`]. Pure.

use clap::Parser;

use crate::types::Config;

/// A GitHub Projects board in the terminal.
#[derive(Clone, Debug, Parser, PartialEq, Eq)]
#[command(version, about)]
pub struct Cli {
    /// The organization that owns the project.
    #[arg(long)]
    pub owner: String,
    /// The project number, from its URL (/orgs/<owner>/projects/<number>).
    #[arg(long)]
    pub project: u64,
    /// `owner/name` new issues go into. Default: the repo most cards live in.
    #[arg(long)]
    pub repo: Option<String>,
    /// The project's single-select field that defines the columns.
    #[arg(long, default_value = "Status")]
    pub status_field: String,
    /// The org-level single-select issue field the number keys filter on.
    #[arg(long, default_value = "Priority")]
    pub priority_field: String,
    /// A status column hidden until `D` toggles it. Repeatable; `--hidden ''`
    /// hides nothing.
    #[arg(long = "hidden", value_name = "STATUS", default_values_t = ["Done".to_string()])]
    pub hidden: Vec<String>,
    /// Start filtered to this priority, by name (any case).
    #[arg(long)]
    pub priority: Option<String>,
}

impl From<Cli> for Config {
    fn from(cli: Cli) -> Self {
        Config {
            owner: cli.owner,
            project: cli.project,
            repo: cli.repo,
            status_field: cli.status_field,
            priority_field: cli.priority_field,
            hidden_statuses: cli.hidden.into_iter().filter(|s| !s.is_empty()).collect(),
            priority: cli.priority,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(args: &[&str]) -> Config {
        let argv = std::iter::once("gh-board").chain(args.iter().copied());
        Cli::try_parse_from(argv).expect("valid args").into()
    }

    #[test]
    fn defaults_match_the_lua_plugin() {
        assert_eq!(
            config(&["--owner", "acme", "--project", "7"]),
            Config::new("acme", 7)
        );
    }

    #[test]
    fn every_option_maps_onto_the_config() {
        let c = config(&[
            "--owner",
            "acme",
            "--project",
            "7",
            "--repo",
            "acme/plans",
            "--status-field",
            "Stage",
            "--priority-field",
            "Urgency",
            "--hidden",
            "Done",
            "--hidden",
            "Archived",
            "--priority",
            "high",
        ]);
        assert_eq!(c.repo.as_deref(), Some("acme/plans"));
        assert_eq!(c.status_field, "Stage");
        assert_eq!(c.priority_field, "Urgency");
        assert_eq!(c.hidden_statuses, ["Done", "Archived"]);
        assert_eq!(c.priority.as_deref(), Some("high"));
    }

    #[test]
    fn an_empty_hidden_hides_nothing() {
        let c = config(&["--owner", "acme", "--project", "7", "--hidden", ""]);
        assert!(c.hidden_statuses.is_empty());
    }

    #[test]
    fn owner_and_project_are_required() {
        assert!(Cli::try_parse_from(["gh-board", "--owner", "acme"]).is_err());
        assert!(Cli::try_parse_from(["gh-board", "--project", "7"]).is_err());
    }
}
