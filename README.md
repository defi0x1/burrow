# burrow

*A disposable hole for your agent to work in. Isolated worktree, hard limits, and a receipt when it is done.*

[![tests](https://github.com/defi0x1/burrow/actions/workflows/test.yml/badge.svg)](https://github.com/defi0x1/burrow/actions/workflows/test.yml)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![rust](https://img.shields.io/badge/rust-2021-orange.svg)](Cargo.toml)

Coding agents now run unattended and in parallel, and every one of them
writes directly into your working tree. There is no isolation between runs,
no limit on how much a run can touch, and no record of what it actually did.
burrow gives any command — agent or not — a worktree of its own, hard limits
on what it's allowed to change, and a verdict when it finishes.

## What it looks like

A real run, wrapping a script that fixes an off-by-one in a pagination helper:

```
burrow 2026-09-03T03-25-10-4df6  ·  from HEAD (f8c9588)

  $ ./fix-pagination.sh

  exit 0 · 0s

  CHANGED   3 files · +11 −2
    CHANGELOG.md             +1 −1
    src/pagination.ts        +1 −1
    src/pagination.test.ts   +9 −0

  GUARDS    files 3/10 · bytes 341B/64K · denylist clean · no secrets

  ok        burrow promote 2026-09-03T03-25-10-4df6 --branch <name>
            burrow rm 2026-09-03T03-25-10-4df6
```

And a real blocked run, wrapping a script that touches a workflow file and
writes more files than the limit allows:

```
  GUARDS    files 13/10 · denylist HIT

  blocked   changed .github/workflows/release.yml (denylist)
            3 more files than the limit allows
            the hole is kept so you can look: burrow show 2026-09-03T03-25-22-7b69
```

Both are unedited output from real runs against a scratch repo, captured
while building this tool.

## Install

burrow is not published to crates.io. Install it straight from the repo:

```
cargo install --git https://github.com/defi0x1/burrow
```

Or build from source:

```
git clone https://github.com/defi0x1/burrow
cd burrow
cargo build --release
# binary at target/release/burrow
```

## Quickstart

```
burrow run -- sh -c 'echo hi > note.txt'
burrow ls
burrow promote <id> --branch try/note
```

The first command wraps a harmless shell command so you can see the whole
flow without an agent installed. Swap it for whatever you actually want
isolated — `claude -p "..."`, a codegen script, a build step, anything that
writes to the filesystem.

## How it works

1. Resolve the repository root. burrow refuses to run outside a git repo.
2. Create the hole: `git worktree add --detach <state>/burrows/<id> <ref>`,
   where `<ref>` defaults to `HEAD`. The id is a timestamp plus a short
   random suffix — sortable, human-readable, good enough to type from memory.
3. Spawn the command with its working directory set to the burrow, stdio
   inherited, and `BURROW_ID`, `BURROW_DIR`, `BURROW_REF` set in its
   environment.
4. Enforce the wall-clock timeout, if one was given. The command runs in its
   own process group; on timeout burrow signals the whole group (SIGTERM,
   then SIGKILL after a grace period) so orphaned grandchildren die with it.
5. When the command exits, burrow inspects the worktree with `git status`
   and `git diff`: changed files, added and removed lines, total added
   bytes.
6. Four guards run, in order — see [docs/guards.md](docs/guards.md).
7. The verdict is `ok` or `blocked`. A receipt is written to
   `.burrow/meta/<id>/receipt.json`; `--json` prints it to stdout.
8. Retention: `--keep` always keeps the hole, `--discard` always removes it.
   The default keeps it on both outcomes — a blocked burrow is exactly the
   one you want to inspect — and prints the `burrow rm` command to clean up.
9. `burrow promote` commits the burrow's changes and either creates a branch
   in the main repo or writes a patch file. It refuses to promote a blocked
   burrow unless you pass `--force`. See
   [docs/promoting.md](docs/promoting.md).
10. Burrows are parallel-safe. Each has its own lock file; `ls` and
    `rm --stale` detect a lock whose process has died instead of hanging on
    it.

## Guards

Evaluated in this order. Full detail, including exactly what trips each one,
is in [docs/guards.md](docs/guards.md).

| Guard | Default |
|---|---|
| Denylist glob match on any changed path | `.env*`, `**/*.pem`, `**/id_rsa*`, `**/secrets/**`, `.github/workflows/**`, common lockfiles, `**/migrations/**` |
| Changed file count | 10 |
| Added bytes | 65536 (64K) |
| Secret-shaped content in added lines | private key headers, AWS access key ids, high-entropy values near `token`/`secret`/`key`/`password`/`api_key` |

## Configuration

Optional `burrow.json` at the repository root. Flags always override it.

```json
{
  "max_files": 15,
  "max_bytes": 131072,
  "deny": [
    "**/*.pem",
    "**/secrets/**",
    "**/migrations/**"
  ]
}
```

`max_files` and `max_bytes` replace the built-in default when set. `deny`
replaces the built-in denylist entirely — list everything you want blocked,
not just your additions. `--deny` on the command line is always additive on
top of whichever list applies.

## Works with loop-rat

burrow is the isolation primitive a [loop-rat](https://github.com/mrbuzzoni/loop-rat)
shift needs. loop-rat fingerprints your working tree and blocks a shift from
writing to it directly; burrow gives that shift a worktree of its own to
legitimately write into, with limits and a receipt instead of a bare
rejection. They're standalone tools — burrow doesn't depend on loop-rat and
doesn't know it exists — but the combination is: loop-rat stops agents from
touching your tree, burrow gives them somewhere to go instead.

## Limitations

- Unix only. Process-group signalling (the timeout mechanism) uses `libc`
  directly and has no Windows path.
- A git worktree is not a container. It isolates the checkout, not the
  filesystem outside the repo, not the network, and not environment
  variables the command inherits. burrow is not a security boundary — do
  not run untrusted code in a burrow expecting it to be contained.
- Submodules and generated build artifacts need care. A command that writes
  large build output into the worktree will trip the byte/file guards the
  same as any other change; burrow does not special-case `.gitignore`.
- Secret detection is heuristic pattern matching, not a real secrets
  scanner. It will miss real secrets that don't match its shapes, and it
  will flag things that aren't secrets. Treat a hit as "look here," not as
  a guarantee either way.

## Development

```
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
tests/smoke.sh
```

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Related

Sibling tools built on the same shape: a deterministic oracle does the scoring,
and a model is only ever allowed to write the prose.

- [mole](https://github.com/defi0x1/mole) -- plants plausible bugs in your code and reports which ones your tests failed to catch
- [drift-graph](https://github.com/defi0x1/drift-graph) -- snapshots your import graph and shows which architectural boundaries got crossed
- [hindsight](https://github.com/defi0x1/hindsight) -- replays an EVM wallet's trades against what would have happened if it had done nothing

## License

MIT, see [LICENSE](LICENSE).
