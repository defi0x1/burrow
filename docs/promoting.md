# Promoting

A burrow's worktree is disposable by construction. `burrow promote` is the
one path that turns it into something durable: a commit, landed as a branch
or written out as a patch.

## What it does

```
burrow promote <id> [--branch <name>] [--patch <file>] [--force]
```

1. Commits everything currently in the burrow's worktree (`git add -A` +
   `git commit`), inside the worktree.
2. Either creates a branch in the main repository pointing at that commit
   (default: `burrow/<id>`, or `--branch <name>`), or writes the commit out
   as a patch file with `--patch <file>`.

The main repository's working tree is never touched. Promoting only adds a
branch ref (or a file on disk for `--patch`) — it does not check anything
out.

## Blocked burrows

`promote` refuses a burrow whose verdict is `blocked`:

```
$ burrow promote 2026-09-03T14-31-02-b71c
burrow: burrow 2026-09-03T14-31-02-b71c is blocked: changed .github/workflows/release.yml (denylist).
use --force to promote anyway
```

Pass `--force` to promote it anyway. This is a deliberate override, not a
default — read `burrow show <id>` first and decide whether the guard was
right.

`--force` also lets `--branch` overwrite an existing branch of the same name.

## Choosing branch vs. patch

Use `--branch` when you want the result checked out normally:

```
burrow promote 2026-09-03T14-22-11-a3f9 --branch fix/pagination
git checkout fix/pagination
```

Use `--patch` when you want to review the diff before it touches your repo
at all, or when you're moving the change somewhere else (another clone, a
review tool, an email):

```
burrow promote 2026-09-03T14-22-11-a3f9 --patch pagination.patch
git apply pagination.patch
```

## After promoting

The burrow itself still exists — promoting does not remove it. Run
`burrow rm <id>` once you've confirmed the branch or patch is what you
wanted.
