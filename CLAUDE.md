# CLAUDE.md

## gh-board

A GitHub Projects board in the terminal, in Rust with ratatui. `README.md`
describes the intended behaviour; keep it in step with the keys and options.

GitHub API specifics worth knowing before touching `queries.rs`/`decode.rs`:

- Priority is an **org-level issue field**, not a project field: it is read
  from and written to the issue, and `--priority-field` names it.
- The project's own workflows react to closing an issue or changing its status
  a few seconds later, outside our response (closing moves the item to Done;
  Done closes the issue). So a confirmed state or status change re-fetches the
  item after a delay (`update.rs`, `fetch_item`).
- Every request goes to `gh api graphql --input -` as JSON on stdin.

Runtime requirement: **`gh` on `PATH`**, logged in with `project`, `read:org`
and `repo` scopes. The app never handles a token.

## Architecture: functional core, imperative shell

```
src/types.rs    domain types, already decoded (Card, Field, Meta, Board, Config...)
src/model.rs    State, Op, Focus; derived views: the fold, columns, focus, target repo
src/update.rs   the reducer: update(State, Msg) -> (State, Vec<Effect>)
src/search.rs   fuzzy card search (per-field subsequence, smart case, 'exact)
src/queries.rs  GraphQL documents + variables -> Request (serialises to gh's stdin JSON)
src/decode.rs   GraphQL `data` (serde_json::Value) -> domain types
src/cli.rs      clap options -> Config
src/ui/         the UI's pure parts (no terminal, no clock: `now` is passed in):
  state.rs      UiState (mode, pending `g`, failure prompt, notice, board and
                detail scroll), apply_effect/notify/expire, settle (scroll to
                focus, clamp the detail scroll)
  keys.rs       handle_key(&UiState, &State, KeyEvent) -> (UiState, Outcome
                { msgs, actions }); Action = Quit/OpenUrl/Redraw/Notify/Edit;
                board, detail, picker, prompt and quit-guard keys; HELP
  render.rs     render(frame, &State, &UiState): headers, columns, message
                line, status line; detail, picker, help and failure overlays
  detail.rs     the detail view's lines, pre-wrapped to a width; its box; HELP
  picker.rs     Picker (kind, item, cursor) -> menu(): choices derived from
                the live state each frame, each carrying its Msg
  editor.rs     $VISUAL/$EDITOR/vi command; decide(edit, success, text) -> Msg
  card.rs       a card's content lines, height and border style
  layout.rs     column widths, sideways scroll, scroll-to-focused-card
  text.rs       display-width fitting (unicode-width), word split, wrap
  theme.rs      colours
src/shell/      the impure part of the library:
  gh.rs         Transport trait; Gh runs `gh api graphql --input -`; parse()
  effects.rs    perform(): one network effect -> requests -> decoded Msgs
                (incl. the three-step, resumable create); Runner runs it on a
                thread and sends Msgs down an mpsc channel
  app.rs        the app loop: settle + draw, poll keys, drain the channel;
                dispatch = update, then Runner::run each effect and apply the
                UI effects it hands back; browser (open/xdg-open), clipboard
                (pbcopy; wl-copy/xclip elsewhere), and suspending the terminal
                for the editor
  editor.rs     run the editor on an exclusive temp .md file, read it back,
                remove it
src/main.rs     parse the options, ratatui::run(app::run)
examples/       requests.rs (every request, for check-schema), load_board.rs
scripts/check-schema.sh   validate every document against GitHub's live schema
```

UI-only state lives in `ui::UiState`, never in `model::State`. Each frame
the loop calls `UiState::settle(&State, area)` (pure: scrolls so the focused
card is whole on screen, columns by whole cards) and then `render`, which
uses the offsets as they are. Keys go through `handle_key`; its messages go
through `update` like any other, and its `Action`s are what only the shell can
do. A failure prompt waits while a text prompt (`/`, `n`, `F`) or the quit
question is open.

The detail view is open exactly when `state.detail` is set (the reducer owns
that); only its scroll position is UI state. Pickers store no choices: they
are re-derived from the state, so the assignee picker's ticks follow each
optimistic toggle. `q`/`Ctrl-c` with queued or in-flight ops asks before
quitting (failed ops do not count).

`queries.rs` documents are checked against GitHub's live schema by
`scripts/check-schema.sh`; run it after changing any of them. Keep fragments per document exact: GraphQL rejects
a document with an unused fragment, so `with_issue`/`with_item` attach only
what the document spreads.

Everything outside `shell/` is pure: no I/O, no terminal, no clock. Anything that needs the
outside world is an `Effect`; its outcome comes back as a `Msg`.

Conventions:

- **Messages carry decoded data.** The shell decodes GraphQL responses and
  sends `BoardLoaded { board }`, `OpOk { result: Result<MutationResult, _> }`
  and so on. A decode failure becomes the corresponding `*Failed` message (or
  `OpOk` with `Err`, meaning "GitHub accepted it but the response was wrong").
- Indexes are 0-based; "nothing there" is `Option`, not 0.
- `update` takes `State` by value and handlers mutate it in place: callers
  that need the old state clone it.

### Optimism is a fold

What the board shows is never stored. `model::view` derives it as `base` (last
server truth) with every unconfirmed op in `state.ops` applied in order.
Confirming merges the server's card into `base` and drops the op; abandoning
drops the op, which *is* the rollback. Do not add "previous value"
bookkeeping — if a change seems to need it, it belongs in the fold.

Ops per card are serialised (`Inflight`/`Queued`); a success supersedes older
failed ops of the same kind on the same card; a retry re-enqueues with a fresh
id. `confirms`/`confirmed_at` stop a fetch already in flight from overwriting a
card confirmed after it began.

## Commands

```bash
cargo test                     # everything
cargo test --test props        # property tests only
PROPTEST_CASES=5000 cargo test --test props
cargo clippy --all-targets -- -D warnings
cargo fmt
scripts/check-schema.sh      # after changing any query (talks to GitHub; no side effects)
cargo run --example load_board -- <owner> <project>   # live read-only load
cargo run -- --owner <org> --project <n>              # the app
cargo insta test && cargo insta review                # snapshot changes
```

Use `nix develop -c <cmd>` outside direnv. Always run `cargo fmt`, clippy and
`cargo test` before declaring work done.

## Tests

- `tests/update.rs` — the reducer, as data.
- `tests/props.rs` — proptest: random users against a simulated GitHub that
  answers in any order and sometimes fails. Checks per-card serialisation,
  intent-ordered sends, in-flight ops == outstanding requests, base == GitHub,
  the view always equals the user's intent while GitHub succeeds, and abandon
  is a rollback. When adding an op kind, teach `Sim::respond` about it.
- `tests/decode.rs` — decoders, including a proptest round trip (any `Card`
  -> API JSON -> decode == the card) and a no-panic check on arbitrary JSON.
- `tests/queries.rs` — documents, variables, and op -> request mapping.
- `tests/shell.rs` — `parse`; `Gh` against `tests/fixtures/echo-gh`, which
  answers with its argv and stdin; `perform` against a scripted `Fake`
  transport, including create-resume driven through the real reducer
  (`drive`); `Runner`.
- `tests/ui_keys.rs` — key handling as data (keys -> msgs/actions/UI
  changes), driven through the real reducer where it matters: search prompt
  (live, Enter keeps, Esc restores), new-issue prompt, failure prompt, help,
  notices, scroll-to-focus.
- `tests/ui_detail.rs` — the detail view, pickers and quit guard: keys as
  data through the reducer (scrolling clamps via `settle`), and snapshots
  (loading, loaded, failed load, scrolled long body, live pending/failed
  marks, pickers, help).
- `tests/editor.rs` — `shell::editor::edit` against
  `tests/fixtures/echo-editor` (initial text, `.md` file, argv, `:cq`, temp
  file removed), then `ui::editor::decide` through the reducer (comments,
  descriptions, and feedback, whose `:cq` cancels sending).
- `tests/ui_render.rs` — insta snapshots of `render` on a 100x30
  `TestBackend` (`tests/snapshots/`); border and priority colours are
  asserted on buffer cells since snapshots carry no colour. After a
  deliberate rendering change: `cargo insta test`, read every `.snap.new`,
  then `cargo insta accept`.
- Unit tests in `src/ui/{text,layout,card,render,detail,editor}.rs` and
  `src/cli.rs`.
- `tests/common/mod.rs` — synthetic data; `tests/common/gh.rs` encodes domain
  types into GitHub's response shapes. **No real project data in fixtures**:
  real boards are usually private.
- Fake executables are committed in `tests/fixtures/`, never written by a
  test: writing a script then running it races with other threads' forks
  on Linux (ETXTBSY, "Text file busy").
- `proptest-regressions/` holds seeds of past failures: commit them.

The suite is offline. Besides the app itself, `check-schema.sh` and
`load_board` are the only things that talk to GitHub, and neither changes
anything.

## Version control

Jujutsu (jj), git-colocated. Use `jj`; never git commands.
