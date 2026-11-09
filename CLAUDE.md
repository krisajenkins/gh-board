# CLAUDE.md

## gh-board

A GitHub Projects board in the terminal: a Rust + ratatui port of
`~/Work/Tools/nvim/gh-board.nvim`. That plugin's `README.md` describes the
intended behaviour and its `CLAUDE.md` the GitHub API specifics (priority is an
org-level issue field, project workflows react to close/status out of band,
requests go to `gh api graphql --input -` as JSON on stdin). Read both before
porting a new area; behaviour should match unless there is a reason to differ.

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
src/shell/      the impure part of the library:
  gh.rs         Transport trait; Gh runs `gh api graphql --input -`; parse()
  effects.rs    perform(): one network effect -> requests -> decoded Msgs
                (incl. the three-step, resumable create); Runner runs it on a
                thread and sends Msgs down an mpsc channel
src/main.rs     the app (placeholder for now)
examples/       requests.rs (every request, for check-schema), load_board.rs
scripts/check-schema.sh   validate every document against GitHub's live schema
```

Not yet ported: the ratatui UI and the app loop (dispatch = update, then
hand each effect to `Runner::run`; it returns UI effects for the UI).

`queries.rs` documents are copied verbatim from the Lua plugin, which probed
them against the live API. Keep fragments per document exact: GraphQL rejects
a document with an unused fragment, so `with_issue`/`with_item` attach only
what the document spreads.

Everything outside `shell/` is pure: no I/O, no terminal, no clock. Anything that needs the
outside world is an `Effect`; its outcome comes back as a `Msg`.

Differences from the Lua design:

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
```

Use `nix develop -c <cmd>` outside direnv. Always run `cargo fmt`, clippy and
`cargo test` before declaring work done.

## Tests

- `tests/update.rs` — the reducer, as data; ported from `test_update.lua`.
- `tests/props.rs` — proptest: random users against a simulated GitHub that
  answers in any order and sometimes fails. Checks per-card serialisation,
  intent-ordered sends, in-flight ops == outstanding requests, base == GitHub,
  the view always equals the user's intent while GitHub succeeds, and abandon
  is a rollback. When adding an op kind, teach `Sim::respond` about it.
- `tests/decode.rs` — decoders, including a proptest round trip (any `Card`
  -> API JSON -> decode == the card) and a no-panic check on arbitrary JSON.
- `tests/queries.rs` — documents, variables, and op -> request mapping.
- `tests/shell.rs` — `parse`; `Gh` against a fake `gh` script (argv, stdin);
  `perform` against a scripted `Fake` transport, including create-resume
  driven through the real reducer (`drive`); `Runner`.
- `tests/common/mod.rs` — synthetic data; `tests/common/gh.rs` encodes domain
  types into GitHub's response shapes. **No real project data in fixtures**;
  the board is private.
- `proptest-regressions/` holds seeds of past failures: commit them.

The suite is offline. `check-schema.sh` and `load_board` are the only things
that talk to GitHub, and neither changes anything.

## Version control

Jujutsu (jj), git-colocated. Use `jj`; never git commands.
