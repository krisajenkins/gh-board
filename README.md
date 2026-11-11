# gh-board

A [GitHub Projects](https://docs.github.com/en/issues/planning-and-tracking-with-projects)
board in the terminal: a column-per-status kanban you can filter, search and
edit, with every change shown immediately and confirmed in the background.

Written in Rust with [ratatui](https://ratatui.rs); it talks to GitHub through
the [`gh`](https://cli.github.com) CLI, so it never handles a token itself.

> **Status: early.** It works day to day (the board, filters, search, a card
> detail view, pickers and editing in `$EDITOR`), but keys and options may
> still change. Only organization-owned projects are supported. Press `F` in
> the app to file feedback.

## Requirements

- [`gh`](https://cli.github.com), logged in with the `project`, `read:org` and
  `repo` scopes:

  ```sh
  gh auth login
  gh auth refresh -s project
  ```

## Install

Prebuilt binaries for Linux (x86_64, aarch64; static musl) and macOS (Intel,
Apple silicon) are attached to each
[release](https://github.com/krisajenkins/gh-board/releases). Unpack one and
put `gh-board` on your `PATH`:

```sh
tar xzf gh-board-v*-aarch64-apple-darwin.tar.gz
cp gh-board-v*-aarch64-apple-darwin/gh-board ~/.local/bin/
```

On macOS, a binary downloaded with a browser is quarantined; clear that with
`xattr -d com.apple.quarantine ~/.local/bin/gh-board`.

Or build from source with a Rust toolchain (1.88 or later):

```sh
cargo install --locked --git https://github.com/krisajenkins/gh-board
```

## Usage

```sh
gh-board --owner my-org --project 1
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
| `F`             | Send feedback: files an issue on gh-board's own repository. Type a title, then write an optional description in `$EDITOR` (`:cq` cancels). |
| `D`             | Show / hide the `--hidden` columns.                           |
| `R`             | Reload everything from GitHub.                                |
| `!`             | Review failed changes.                                        |
| `Enter`         | Open the card's detail view.                                  |
| `o`             | Open the card in the browser.                                 |
| `?`             | Key help.                                                     |
| `q` `Ctrl-c`    | Quit. Nothing is saved. If changes are still being sent, asks first. |

In the search, new-issue and feedback prompts, `Backspace`, `Ctrl-w` and
`Ctrl-u` edit.

### Search

Search is fuzzy, in the style of fzf. Each card is searched on what it shows:
`#number`, title, `@assignee` for each assignee, each label, and the priority.

- Words are separated by spaces, and a card must match every word.
- A word matches a field when its letters appear in it in order, not
  necessarily together: `rlnts` matches "Release notes".
- Each word must match within one field: `alice blog` can match an assignee
  and a label, but a single word never spans two fields.
- Smart case: an all-lower-case word ignores case; a word with a capital in it
  is case-sensitive.
- A leading `'` matches exactly: `'idc` matches "idc" but not "in dev
  channel".

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
| `F`             | Send feedback about gh-board itself (as on the board).        |
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
