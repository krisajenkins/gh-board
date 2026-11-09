# gh-board

A [GitHub Projects](https://docs.github.com/en/issues/planning-and-tracking-with-projects)
board in the terminal: a column-per-status kanban you can filter, search and
edit, with every change shown immediately and confirmed in the background.

A port of [gh-board.nvim](../nvim/gh-board.nvim) to Rust and
[ratatui](https://ratatui.rs).

> **Status: early.** Everything in the Neovim plugin is here: the board,
> filters, search, the card detail view, pickers and editing in `$EDITOR`.

## Requirements

- [`gh`](https://cli.github.com), logged in with the `project`, `read:org` and
  `repo` scopes:

  ```sh
  gh auth login
  gh auth refresh -s project
  ```

## Usage

```sh
cargo run --release -- --owner my-org --project 1
```

| Option             | Default    | Meaning                                                       |
| ------------------ | ---------- | ------------------------------------------------------------- |
| `--owner`          | required   | The organization that owns the project.                       |
| `--project`        | required   | Project number (`/orgs/<owner>/projects/<number>`).           |
| `--repo`           | (most used) | `owner/name` new issues go into.                             |
| `--status-field`   | `Status`   | The project's single-select field that defines the columns.   |
| `--priority-field` | `Priority` | The org-level single-select issue field the number keys use.  |
| `--hidden`         | `Done`     | A column hidden until `D`. Repeatable; `--hidden ''` for none. |
| `--priority`       | (all)      | Start filtered to this priority, by name (any case).          |

Each column is a status; each card a rounded box with its number and title,
then its priority, `@assignees` and labels. The focused card has a cyan
border. `~` and a blue border mark a change not yet confirmed by GitHub; `!`
and a red border one that failed. Long columns scroll (the header shows how
many cards are above `↑` and below `↓`), and when the columns do not all fit
the board scrolls sideways.

| Key             | Action                                                        |
| --------------- | ------------------------------------------------------------- |
| `h j k l`       | Move focus (arrows work too). `gg` / `G`: top / bottom.       |
| `0`             | Show all priorities.                                          |
| `1`..`n`        | Show one priority, in field order (Urgent, High, ...).        |
| `n+1`           | Show cards with no priority (`5` with four options).          |
| `/`             | Fuzzy search, filtering as you type. `Enter` keeps it, `Esc` undoes. |
| `Esc` `Ctrl-l`  | Clear the search (`Ctrl-l` also redraws).                     |
| `<` / `>`       | Move the card to the previous / next status column.           |
| `n`             | New issue in the focused column, at the filtered priority.    |
| `D`             | Show / hide the `--hidden` columns.                           |
| `R`             | Reload everything from GitHub.                                |
| `!`             | Review failed changes.                                        |
| `Enter`         | Open the card's detail view.                                  |
| `o`             | Open the card in the browser.                                 |
| `?`             | Key help.                                                     |
| `q` `Ctrl-c`    | Quit. Nothing is saved. If changes are still being sent, asks first. |

In the search and new-issue prompts, `Backspace`, `Ctrl-w` and `Ctrl-u` edit.
Search syntax is as in
[gh-board.nvim](../nvim/gh-board.nvim#search): space-separated fuzzy words
matched against `#number`, title, `@assignee`, labels and priority; a capital
makes a word case-sensitive; a leading `'` matches exactly.

### The detail view

`Enter` opens the card's fields, description and latest 50 comments (oldest
first). It follows the board live: a change shows at once, marked `~` until
GitHub confirms it or `!` if it failed.

| Key             | Action                                                        |
| --------------- | ------------------------------------------------------------- |
| `j k` `Ctrl-d` `Ctrl-u` | Scroll; `gg` / `G` top / bottom.                      |
| `e`             | Edit the description in your editor.                          |
| `c`             | Write a comment in your editor.                               |
| `s` / `p`       | Status / priority picker (priority has a "None").             |
| `a`             | Assignee picker: each `Enter` toggles a user; `Esc` closes.   |
| `m` / `u`       | Assign me / unassign everyone.                                |
| `x`             | Close (Completed / Not planned), or reopen a closed issue.    |
| `<` `>` `!`     | As on the board.                                              |
| `o`             | Open in the browser.                                          |
| `r`             | Reload.                                                       |
| `q` `Esc`       | Close the detail view.                                        |

Pickers: `j`/`k` or arrows to move, `Enter` to pick, `Esc` to cancel; the
current value is marked `●`, assigned users `✓`.

The editor is `$VISUAL`, else `$EDITOR`, else `vi`, run on a temporary `.md`
file. Save and quit to send; quit with an error (`:cq` in Vim) to discard. An
empty comment is not posted.

When a change fails, a prompt offers `r` Retry, `a` Abandon (the card goes
back to how GitHub has it), or `Esc` to decide later (`!` brings it back).

## Development

```sh
nix develop      # or direnv
cargo test
cargo clippy --all-targets -- -D warnings
```

See `CLAUDE.md` for the architecture.
