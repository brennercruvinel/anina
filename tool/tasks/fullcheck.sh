#!/bin/sh
# every gate a pull request must pass, in order. ci runs this same script
# (.github/workflows/gatecheck.yml), so what passes here is what passes there:
# a gate is added in this file and nowhere else.
# run from anywhere: `tool/tasks/fullcheck.sh`
set -eu
cd "$(dirname "$0")/../.."

# one gate: its name, then its command. a gate that fails ends the script
# (`set -e`), and `finish` says which one it was. in GitHub Actions a gate is
# also a group in the log and a row in the job's summary.
gate() {
  running=$1
  shift
  if [ -n "${GITHUB_ACTIONS:-}" ]; then echo "::group::$running"; else echo "fullcheck: $running" >&2; fi
  started=$(date +%s)
  "$@"
  [ -z "${GITHUB_ACTIONS:-}" ] || echo "::endgroup::"
  row "$running" passed "$(($(date +%s) - started))s"
  running=
}
row() {
  [ -z "${GITHUB_STEP_SUMMARY:-}" ] || echo "| $1 | $2 | $3 |" >>"$GITHUB_STEP_SUMMARY"
}
finish() {
  release_lock
  [ -n "$running" ] || return 0
  row "$running" "**failed**" ""
  [ -z "${GITHUB_ACTIONS:-}" ] || printf '::endgroup::\n::error title=fullcheck::the %s gate failed\n' "$running"
  echo "fullcheck: the $running gate failed" >&2
}
running=
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  printf '### fullcheck on %s\n\n| gate | result | time |\n| --- | --- | --- |\n' "$(uname -s)" >>"$GITHUB_STEP_SUMMARY"
fi

# the only warnings expected from packaging: the `leaks`, `walls` and `codes`
# tests are not in the package.
# a target dir under a new path on every run. once packaged, the workspace's
# crates are registry crates to cargo: it unpacks each one into
# ~/.cargo/registry/src/<hash of the registry's path>/ and never again at the
# same version. under a path used before, emptying the dir is not enough: the
# check builds against what the earlier run packaged.
# the last run's dir goes first; this one stays until the next run.
# those unpacked copies are never read again: on the way out, worked or
# failed, every such folder that holds anina crates only goes (this run's and
# any an earlier run left), and other projects' are left alone.
# that folder is one for the whole machine, and anina has several worktrees:
# packaging and the cleanup run under a lock, one fullcheck at a time, so one
# never removes the copy another is still building against.
unpacked="${CARGO_HOME:-$HOME/.cargo}/registry/src"
lock="$unpacked/.anina-fullcheck.lock"
held=0
forget_packages() {
  for d in "$unpacked"/-*/; do
    [ -d "$d" ] || continue
    ls "$d" | grep -qv '^anina-' && continue
    rm -rf "$d"
  done
}
# mkdir is atomic: whoever creates the folder holds the lock. the owner's pid
# goes inside, so a fullcheck killed before its trap ran doesn't hold it forever.
take_lock() {
  mkdir -p "$unpacked"
  said=0
  until mkdir "$lock" 2>/dev/null; do
    owner=$(cat "$lock/pid" 2>/dev/null || true)
    if [ -n "$owner" ] && ! kill -0 "$owner" 2>/dev/null; then
      rm -rf "$lock"
      continue
    fi
    if [ "$said" = 0 ]; then
      echo "fullcheck: waiting for the fullcheck in pid ${owner:-?} to finish packaging" >&2
      said=1
    fi
    sleep 1
  done
  held=1
  echo $$ >"$lock/pid"
}
release_lock() {
  [ "$held" = 1 ] || return 0
  forget_packages
  rm -rf "$lock"
  held=0
}
package() {
  take_lock
  rm -rf target/fullcheck-package
  CARGO_TARGET_DIR="target/fullcheck-package/$(date +%s)" cargo package --workspace --locked --allow-dirty
}
trap finish EXIT
trap 'exit 130' INT TERM

gate fmt cargo fmt --check
gate clippy cargo clippy --workspace --all-targets -- -D warnings
gate test cargo test --workspace
gate doctor cargo run --quiet -p anina -- doctor
# anina on its own output: the shell preset against the run kept in tool/tests/shell
gate selfcheck tool/tasks/selfcheck.sh
gate deny cargo deny check
gate package package
