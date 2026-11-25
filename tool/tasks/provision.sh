#!/bin/sh
# what the gates need on a CI runner besides Rust: tmux (the optional engine
# and its test) and, on Linux, bubblewrap (the strict sandbox and its tests).
# every job of .github/workflows/gatecheck.yml that runs the tests starts here.
#
# it installs packages and, on Linux, lets every program use user namespaces:
# right for a runner that is thrown away, not for a developer's machine, so
# it runs in CI only.
set -eu
if [ -z "${CI:-}" ]; then
  echo "provision: this changes the machine and is meant for a CI runner (CI is not set)" >&2
  exit 1
fi
case "$(uname -s)" in
  Linux)
    sudo apt-get install -y tmux bubblewrap
    # bwrap needs unprivileged user namespaces; Ubuntu 24.04 keeps them behind AppArmor
    if [ -e /proc/sys/kernel/apparmor_restrict_unprivileged_userns ]; then
      sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0
    fi
    ;;
  Darwin)
    brew install tmux
    ;;
  *)
    echo "provision: no packages known for $(uname -s)" >&2
    exit 1
    ;;
esac
