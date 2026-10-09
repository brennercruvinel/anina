#!/bin/sh
# every gate a pull request must pass, in the order ci runs them.
# run from anywhere: `tool/tasks/fullcheck.sh`
set -eu
cd "$(dirname "$0")/../.."

cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run --quiet -p anina -- doctor
cargo deny check
# the only warning expected here: the `leaks` test is not in the package.
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
trap release_lock EXIT
trap 'exit 130' INT TERM
take_lock
rm -rf target/fullcheck-package
CARGO_TARGET_DIR="target/fullcheck-package/$(date +%s)" cargo package --workspace --locked --allow-dirty
