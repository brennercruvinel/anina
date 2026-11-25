#!/bin/sh
# the pieces of a release, as the release workflow runs them
# (.github/workflows/release.yml) and as anyone can run them by hand.
#
#   tool/tasks/release.sh version           the workspace's version, from Cargo.toml
#   tool/tasks/release.sh pack TAG TARGET   dist/anina-TAG-TARGET.tar.gz and its .sha256,
#                                           from target/TARGET/release/anina
#   tool/tasks/release.sh notes TAG         the CHANGELOG section of TAG's version
#
# an archive holds the binary, the license and the README. the checksum file
# is the line `shasum -a 256 -c` and `sha256sum -c` read.
# run from anywhere.
set -eu
cd "$(dirname "$0")/../.."

version() {
  sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1
}

# a tag is `v` and the workspace's version: one version, one tag (release.toml)
same_version() {
  if [ "$1" != "v$(version)" ]; then
    echo "release: the tag is $1 and the workspace is at $(version)" >&2
    exit 1
  fi
}

sum() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"; else shasum -a 256 "$1"; fi
}

case "${1:-}" in
  version)
    version
    ;;
  pack)
    tag=${2:?the tag, as v0.1.0}
    target=${3:?the target triple}
    same_version "$tag"
    bin="target/$target/release/anina"
    if [ ! -x "$bin" ]; then
      echo "release: $bin is not built: cargo build --release --locked -p anina --target $target" >&2
      exit 1
    fi
    name="anina-$tag-$target"
    rm -rf "dist/$name"
    mkdir -p "dist/$name"
    cp "$bin" LICENSE README.md "dist/$name/"
    (cd dist && tar -czf "$name.tar.gz" "$name" && sum "$name.tar.gz" >"$name.tar.gz.sha256")
    rm -rf "dist/$name"
    echo "release: dist/$name.tar.gz" >&2
    ;;
  notes)
    tag=${2:?the tag, as v0.1.0}
    same_version "$tag"
    notes=$(awk -v v="## $(version)" '$0 == v {on = 1; next} /^## / {on = 0} on' docs/CHANGELOG)
    if [ -z "$notes" ]; then
      echo "release: docs/CHANGELOG has no section \"## $(version)\"" >&2
      exit 1
    fi
    printf '%s\n' "$notes"
    ;;
  *)
    echo "usage: tool/tasks/release.sh version | pack TAG TARGET | notes TAG" >&2
    exit 2
    ;;
esac
