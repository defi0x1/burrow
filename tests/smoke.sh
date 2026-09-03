#!/usr/bin/env bash
# End-to-end smoke test: builds burrow and exercises the CLI against a
# scratch git repo. Exits non-zero on the first failure.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "==> building"
cargo build --release --quiet
BIN="$ROOT_DIR/target/release/burrow"

WORK="$(mktemp -d)"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

REPO="$WORK/scratch-repo"
mkdir -p "$REPO"
cd "$REPO"
git init -q
git config user.email smoke@example.com
git config user.name smoke
echo "hello" > README.md
mkdir src
echo "fn main() {}" > src/main.rs
git add -A
git commit -qm "init"

pass() { echo "  ok: $1"; }
fail() { echo "  FAIL: $1"; exit 1; }

echo "==> doctor"
"$BIN" doctor | grep -q "burrow is ready." || fail "doctor should report ready"
pass "doctor reports ready inside a repo"

echo "==> doctor outside a repo"
( cd "$WORK" && ! "$BIN" doctor > /tmp/burrow-smoke-doctor.$$ 2>&1 ) || fail "doctor should fail outside a repo"
grep -q "FAIL" /tmp/burrow-smoke-doctor.$$ || fail "doctor should print a FAIL line outside a repo"
rm -f /tmp/burrow-smoke-doctor.$$
pass "doctor fails outside a repo"

echo "==> run: harmless command, ok verdict"
OUT="$("$BIN" run -- sh -c 'echo hi > note.txt')"
echo "$OUT" | grep -q "CHANGED   1 file" || fail "expected 1 changed file"
echo "$OUT" | grep -q "ok        burrow promote" || fail "expected ok verdict"
pass "run produces an ok verdict for a harmless change"

echo "==> ls shows the burrow"
LS="$("$BIN" ls)"
echo "$LS" | grep -q "ok" || fail "ls should show the ok burrow"
ID="$(echo "$LS" | tail -1 | awk '{print $1}')"
[ -n "$ID" ] || fail "could not extract burrow id"
pass "ls lists $ID"

echo "==> show prints the receipt"
"$BIN" show "$ID" | grep -q "CHANGED" || fail "show should print CHANGED block"
pass "show prints the receipt"

echo "==> promote creates a branch"
"$BIN" promote "$ID" --branch smoke/promoted > /tmp/burrow-smoke-promote.$$ 2>&1 || {
  cat /tmp/burrow-smoke-promote.$$; fail "promote should succeed"
}
rm -f /tmp/burrow-smoke-promote.$$
git branch --list smoke/promoted | grep -q smoke/promoted || fail "branch should exist"
[ "$(git show smoke/promoted:note.txt)" = "hi" ] || fail "promoted branch should contain the change"
[ ! -e "$REPO/note.txt" ] || fail "the main working tree must not be touched"
pass "promote lands the change on a branch without touching the main tree"

echo "==> rm cleans up"
"$BIN" rm "$ID" > /dev/null
"$BIN" ls | grep -q "no burrows." || fail "burrow should be gone after rm"
pass "rm removes the burrow"

echo "==> run: denylist violation is blocked"
set +e
OUT="$("$BIN" run --discard -- sh -c 'mkdir -p .github/workflows && echo x > .github/workflows/ci.yml')"
CODE=$?
set -e
[ "$CODE" -eq 1 ] || fail "blocked run should exit 1, got $CODE"
echo "$OUT" | grep -q "denylist HIT" || fail "expected denylist HIT"
echo "$OUT" | grep -q "blocked" || fail "expected blocked verdict"
pass "run blocks a denylisted path and exits 1"

echo "==> run: max-files violation is blocked"
set +e
OUT="$("$BIN" run --discard --max-files 1 -- sh -c 'echo a > a.txt && echo b > b.txt')"
CODE=$?
set -e
[ "$CODE" -eq 1 ] || fail "blocked run should exit 1, got $CODE"
echo "$OUT" | grep -q "files 2/1" || fail "expected files 2/1"
pass "run blocks over the file-count limit"

echo "==> run: secret-shaped content is blocked"
set +e
OUT="$("$BIN" run --discard -- sh -c "echo 'aws_key: AKIAABCDEFGHIJKLMNOP' > creds.txt")"
CODE=$?
set -e
[ "$CODE" -eq 1 ] || fail "blocked run should exit 1, got $CODE"
echo "$OUT" | grep -q "possible AWS access key" || fail "expected AWS key detection"
pass "run blocks an AWS-shaped access key"

echo "==> promote refuses a blocked burrow without --force"
"$BIN" run --keep -- sh -c 'mkdir -p .github/workflows && echo x > .github/workflows/ci.yml' > /dev/null || true
BLOCKED_ID="$("$BIN" ls | tail -1 | awk '{print $1}')"
set +e
"$BIN" promote "$BLOCKED_ID" > /dev/null 2>/tmp/burrow-smoke-refuse.$$
CODE=$?
set -e
[ "$CODE" -ne 0 ] || fail "promote should refuse a blocked burrow"
grep -q "blocked" /tmp/burrow-smoke-refuse.$$ || fail "promote should say why it refused"
rm -f /tmp/burrow-smoke-refuse.$$
pass "promote refuses a blocked burrow without --force"

echo "==> promote --force lands a blocked burrow anyway"
"$BIN" promote "$BLOCKED_ID" --branch smoke/forced --force > /dev/null
git branch --list smoke/forced | grep -q smoke/forced || fail "forced branch should exist"
pass "promote --force overrides the guard"

echo "==> rm --all cleans up everything left"
"$BIN" rm --all > /dev/null
"$BIN" ls | grep -q "no burrows." || fail "rm --all should leave nothing behind"
pass "rm --all clears remaining burrows"

echo "==> --json prints a parseable receipt"
JSON_ID_OUT="$("$BIN" run --json --discard -- sh -c 'echo x > j.txt')"
echo "$JSON_ID_OUT" | tail -n +6 | python3 -c "import json,sys; d=json.load(sys.stdin); assert d['verdict']=='ok'" \
  || fail "--json output should be valid JSON with verdict ok"
pass "--json prints a parseable receipt"

echo
echo "all smoke checks passed."
