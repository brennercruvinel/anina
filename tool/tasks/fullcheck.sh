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
unpacked="${CARGO_HOME:-$HOME/.cargo}/registry/src"
forget_packages() {
  for d in "$unpacked"/-*/; do
    [ -d "$d" ] || continue
    ls "$d" | grep -qv '^anina-' && continue
    rm -rf "$d"
  done
}
trap forget_packages EXIT
rm -rf target/fullcheck-package
CARGO_TARGET_DIR="target/fullcheck-package/$(date +%s)" cargo package --workspace --locked --allow-dirty
