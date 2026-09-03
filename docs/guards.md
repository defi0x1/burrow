# Guards

`burrow run` inspects the worktree after the command exits and runs four
guards, in this order. The first guard to match does not stop the others —
all four run, and every reason that fires is reported. A burrow is `blocked`
if any guard fires.

## 1. Denylist

Every changed path (tracked or untracked, modified or new) is matched against
a set of glob patterns. Any match blocks the run.

Defaults:

```
.env*
**/*.pem
**/id_rsa*
**/secrets/**
.github/workflows/**
**/package-lock.json
**/yarn.lock
**/pnpm-lock.yaml
**/Cargo.lock
**/Gemfile.lock
**/poetry.lock
**/composer.lock
**/migrations/**
```

Configure with `--deny <glob>` (repeatable, added on top of the defaults) or
the `deny` array in `burrow.json` (replaces the defaults).

## 2. Max files

Blocks if the number of changed files exceeds the limit. Default: 10.

Configure with `--max-files <n>` or `max_files` in `burrow.json`.

## 3. Max bytes

Blocks if the total bytes added across all changed files exceeds the limit.
Only added content counts — deleting a large file does not trip this guard.
Default: 65536 (64K).

Configure with `--max-bytes <n>` or `max_bytes` in `burrow.json`.

## 4. Secret-shaped content

Scans every added line (not the whole file, just what changed) for content
that looks like a credential:

- a private key header (`-----BEGIN ... PRIVATE KEY-----`)
- an AWS access key id (`AKIA`/`ASIA` followed by 16 more uppercase
  letters or digits)
- a high-entropy value assigned near one of the words `token`, `secret`,
  `key`, `password`, `api_key`

This guard is heuristic. It will miss secrets that don't match these shapes,
and it will flag things that aren't secrets — a long random test fixture next
to the word "token" is a plausible false positive. Treat a hit as "look at
this line," not as proof.

There is no flag to configure this guard. If it's flagging something that
isn't a secret, that path is probably also worth a second look before you
promote it.

## Precedence

Guard configuration is layered: built-in defaults, then `burrow.json`, then
CLI flags. `--deny` is additive at every layer; `--max-files` and
`--max-bytes` each take the most specific value given.
