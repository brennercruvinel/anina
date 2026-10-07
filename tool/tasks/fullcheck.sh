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
# a target dir of its own, emptied first: once packaged, the workspace's
# crates are registry crates to cargo, and cargo never rebuilds or unpacks a
# registry crate again at the same version, so a reused dir verifies against
# what an earlier run packaged. it stays until the next run empties it.
rm -rf target/fullcheck-package
CARGO_TARGET_DIR=target/fullcheck-package cargo package --workspace --locked --allow-dirty
