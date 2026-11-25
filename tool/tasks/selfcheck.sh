#!/bin/sh
# anina on its own output. plays the `shell` preset and compares the run, with
# `anina compare`, to the one kept in tool/tests/shell/<system>. a change in
# what anina records fails here, with the files that differ and, for the
# texts, what changed in them.
#
#   tool/tasks/selfcheck.sh            compare; exits 1 on a difference
#   tool/tasks/selfcheck.sh --update   play again and keep the run as the reference
#
# a pull request that changes what anina records on purpose runs `--update`
# on macOS and on Linux and brings both references with the change.
#
# one reference per system (ADR-0017): the preset runs the system's `sh`,
# bash on macOS and dash on most of Linux, and they leave different things
# behind. a reference holds what `anina compare` reads: the texts, the PNGs
# (by pixels) and the reports. the files it only wants present, because their
# bytes follow the clock (the SVG, the cast, index.html), are kept empty.
# run from anywhere.
set -eu
cd "$(dirname "$0")/../.."

case "${1:-}" in
  "" | --update) ;;
  *)
    echo "usage: tool/tasks/selfcheck.sh [--update]" >&2
    exit 2
    ;;
esac

case "$(uname -s)" in
  Darwin) system=macos ;;
  *) system=$(uname -s | tr '[:upper:]' '[:lower:]') ;;
esac
kept="tool/tests/shell/$system"
# a system whose reference nobody recorded yet holds this file alone
pending="$kept/PENDING"

# everything this run writes, the cache with its sandbox included
work=target/selfcheck
run="$work/run"
rm -rf "$work"
mkdir -p "$work"

anina() {
  cargo run --quiet -p anina -- "$@"
}

ANINA_CACHE="$PWD/$work/cache" anina run shell --out "$run"

if [ "${1:-}" = --update ]; then
  if [ -d "$kept" ] && [ ! -e "$pending" ] && anina compare "$kept" "$run" >/dev/null 2>&1; then
    echo "selfcheck: $kept already matches this run; left as it is" >&2
    exit 0
  fi
  find "$run" -type f \( -name '*.svg' -o -name '*.gz' -o -name '*.html' -o -name '*.cast' \) \
    -exec sh -c ': >"$1"' sh {} \;
  rm -rf "$kept"
  mkdir -p "$(dirname "$kept")"
  cp -R "$run" "$kept"
  echo "selfcheck: this run is now the reference in $kept; commit it with the change that moved it" >&2
  exit 0
fi

if [ -e "$pending" ]; then
  said="no reference for $system yet: the shell preset ran and nothing was compared. run tool/tasks/selfcheck.sh --update on $system and commit $kept"
  [ -z "${GITHUB_ACTIONS:-}" ] || echo "::warning title=selfcheck::$said"
  echo "selfcheck: $said" >&2
  exit 0
fi
if [ ! -d "$kept" ]; then
  echo "selfcheck: $kept is missing. run tool/tasks/selfcheck.sh --update on $system and commit it" >&2
  exit 1
fi

if anina compare "$kept" "$run" >"$work/differences.txt"; then
  exit 0
fi
cat "$work/differences.txt"
# what changed inside each text, without the times compare leaves out
untimed() {
  sed -E 's/\[[0-9]+(\.[0-9]+)?s\]/[-s]/g' "$1"
}
sed -n 's/^differs: \(.*\.txt\)$/\1/p' "$work/differences.txt" | while IFS= read -r text; do
  untimed "$kept/$text" >"$work/kept.txt"
  untimed "$run/$text" >"$work/this.txt"
  diff -u -L "reference: $text" -L "this run: $text" "$work/kept.txt" "$work/this.txt" || true
done
echo "selfcheck: what anina records changed. if that is meant, run tool/tasks/selfcheck.sh --update and commit $kept" >&2
exit 1
