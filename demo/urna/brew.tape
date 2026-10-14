# brew install hoffresearch/urna/urna on a HOME that never ran brew: brew
# taps hoffresearch/homebrew-urna, trusts the formula it was given by its
# full name and installs the release binary. Homebrew writes to its own
# prefix, outside the sandbox, so the tape refuses to start while urna or
# its tap is already there and removes both at the end. USER is the real
# one: Homebrew stops at "user user doesn't exist" under the sandbox's.
Require brew
Set Cols 100
Set Rows 24
Set FPS 8
Set WaitTimeout 300
Set EndHold 3
Set CleanEnv on
Set Env HOME=$HOME_DIR XDG_CONFIG_HOME=$HOME_DIR/.config USER=$USER LOGNAME=$LOGNAME PATH=/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin LANG=en_US.UTF-8 PS1='$ ' HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_ENV_HINTS=1 HOMEBREW_NO_EMOJI=1 HOMEBREW_NO_BOOTSNAP=1
Exec if brew list --formula urna > /dev/null 2>&1 || brew tap | grep -qx hoffresearch/urna; then echo "urna or the hoffresearch/urna tap is already in Homebrew: brew uninstall urna; brew untap hoffresearch/urna" >&2; exit 1; fi

Spawn sh
Wait /^\$$/

Caption "1 · brew install: the tap, the trusted formula, the release binary"
Type "brew install hoffresearch/urna/urna" 0.03
Key Enter
Wait /^\$$/
Sleep 1.6
Snap 01-install

Caption "2 · the binary on PATH"
Type "urna --version" 0.03
Key Enter
Wait /^urna [0-9]/
Wait /^\$$/
Poster
Sleep 2.4
Snap 02-version
Key ctrl+d
Exec brew uninstall --formula urna > /dev/null; brew untap hoffresearch/urna > /dev/null
