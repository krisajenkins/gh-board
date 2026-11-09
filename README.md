# gh-board

A [GitHub Projects](https://docs.github.com/en/issues/planning-and-tracking-with-projects)
board in the terminal: a column-per-status kanban you can filter, search and
edit, with every change shown immediately and confirmed in the background.

A port of [gh-board.nvim](../nvim/gh-board.nvim) to Rust and
[ratatui](https://ratatui.rs).

> **Status: early.** The core (state, reducer, search) is ported and tested;
> the GitHub transport and the terminal UI are not written yet.

## Requirements

- [`gh`](https://cli.github.com), logged in with the `project`, `read:org` and
  `repo` scopes:

  ```sh
  gh auth login
  gh auth refresh -s project
  ```

## Development

```sh
nix develop      # or direnv
cargo test
cargo clippy --all-targets -- -D warnings
```

See `CLAUDE.md` for the architecture.
