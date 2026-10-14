# cargo binstall urna into a CARGO_HOME inside the sandbox: it reads the
# crate on crates.io and takes the prebuilt binary from the release. The
# toolchain is the machine's (RUSTUP_HOME and PATH), so rustup installs
# nothing; the sandbox's bin folder comes first on PATH. A new CARGO_HOME
# asks about telemetry first; the tape says no.
Require cargo
Require cargo-binstall
Set Cols 100
Set Rows 24
Set FPS 8
Set WaitTimeout 240
Set EndHold 3
Set CleanEnv on
Set Env HOME=$HOME_DIR CARGO_HOME=$HOME_DIR/.cargo RUSTUP_HOME=$REAL_HOME/.rustup PATH=$HOME_DIR/.cargo/bin:$PATH LANG=en_US.UTF-8 PS1='$ '

Spawn sh
Wait /^\$$/

Caption "1 · cargo binstall: the crate's release binary, no compile"
Type "cargo binstall urna" 0.03
Key Enter
Wait "Opt in to telemetry?"
Sleep 1
Type "no" 0.08
Key Enter
Wait "continue?"
Sleep 1
Type "yes" 0.08
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
