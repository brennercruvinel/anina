# npm install -g @urna/cli into a prefix inside the sandbox: the package's
# install script fetches the release binary for this platform.
Require npm
Set Cols 100
Set Rows 24
Set FPS 8
Set WaitTimeout 180
Set EndHold 3
Set CleanEnv on
Set Env HOME=$HOME_DIR PATH=$HOME_DIR/.npm-global/bin:$PATH NPM_CONFIG_PREFIX=$HOME_DIR/.npm-global npm_config_cache=$HOME_DIR/.npm npm_config_update_notifier=false LANG=en_US.UTF-8 PS1='$ '

Spawn sh
Wait /^\$$/

Caption "1 · npm install -g: the package and the release binary"
Type "npm install -g @urna/cli" 0.03
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
