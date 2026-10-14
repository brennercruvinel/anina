# uv tool install urna into the sandbox: the wheel from PyPI, whose urna
# command is the read-only subset (validate, inspect, stats, search) over
# the library. Its bin folder is its own, next to the released binary the
# preset puts in ~/.local/bin.
Require uv
Set Cols 100
Set Rows 24
Set FPS 8
Set WaitTimeout 240
Set EndHold 3
Set CleanEnv on
Set Env HOME=$HOME_DIR UV_TOOL_BIN_DIR=$HOME_DIR/.uv/bin PATH=$HOME_DIR/.uv/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin LANG=en_US.UTF-8 PS1='$ '
Set Cwd $URNA_CORPUS_DIR
Exec test -f "$URNA_CORPUS_DIR/quickstart.urna" || { echo "no quickstart.urna in $URNA_CORPUS_DIR: set URNA_REPO (to build it) or URNA_CORPUS_DIR" >&2; exit 1; }

Spawn sh
Wait /^\$$/

Caption "1 · uv tool install: the wheel from PyPI"
Type "uv tool install urna" 0.03
Key Enter
Set FPS 3              # uv redraws its progress bars all the time: fewer samples
Speed 2
Wait /^\$$/
Speed 1
Set FPS 8
Sleep 1.6
Snap 01-install

Caption "2 · validate, from Python: every checksum and hash"
Type "urna validate quickstart.urna" 0.03
Key Enter
Wait /^\$$/
Poster
Sleep 2.4
Snap 02-validate
Key ctrl+d
