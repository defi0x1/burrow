# Contributing

burrow is a small, single-binary Rust tool. Keep changes small and keep the
dependency list short — see the list in `Cargo.toml` before adding a crate.

## Setup

```
cargo build
cargo test
```

## Before you send a change

```
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
tests/smoke.sh
```

All four must pass. CI runs the same four commands on Linux and macOS.

## Code shape

- `src/repo.rs` — everything that shells out to `git`.
- `src/worktree.rs` — burrow ids, on-disk layout, locking.
- `src/run.rs` — spawning the wrapped command, timeout, process group signalling.
- `src/guards.rs` — the four guards and their defaults.
- `src/receipt.rs` — the receipt schema and its serialization.
- `src/report.rs` — the human-readable report shown in the terminal.

If you're adding a guard or changing a default, update `docs/guards.md` in the
same change. If you're changing the receipt schema, update `docs/promoting.md`
where relevant and keep old fields around if you can — the receipt is meant to
be scriptable.

## Tests

Integration tests in `tests/integration.rs` create real git repositories in
temp directories and run the built binary against them. Do not mock git.
`tests/smoke.sh` is the same idea at the shell level, and is what CI treats as
the final gate. Add a test for any new guard or CLI flag.

## Reporting bugs

Open an issue with the command you ran, what you expected, and what
`burrow show <id>` printed. Attach `receipt.json` from `.burrow/meta/<id>/` if
you can — most bugs are guard logic or diff parsing, and the receipt has the
full story.

## Scope

burrow wraps a command in an isolated worktree and reports what changed. It is
not trying to grow into a sandbox, a CI system, or an agent framework. Changes
that add a new top-level concern (not a guard, not a report field) are likely
out of scope — open an issue first to discuss before sending a large patch.
