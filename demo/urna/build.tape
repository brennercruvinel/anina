# urna build from a checkout: the starter corpus (twelve paragraphs, one
# spec) in, one .urna out, then every hash in it checked. The binary is the
# checkout's own target/release/urna when there is one: the forge it runs
# moves with the code, so a released binary may not find it in a newer
# checkout. Needs the checkout's PyO3 extension and a .venv with numpy and
# tokenizers, which tool/tasks/fullcheck.sh leaves there.
Set Cols 100
Set Rows 26
Set FPS 8
Set WaitTimeout 120
Set EndHold 3
Set CleanEnv on
Set Env HOME=$URNA_HOME PATH=$URNA_REPO/target/release:$URNA_BIN:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin URNA_PYTHON=$URNA_REPO/.venv/bin/python LANG=en_US.UTF-8 PS1='$ '
Set Cwd $URNA_REPO
Exec rm -rf "$URNA_REPO/demo/starter/out"

Spawn sh
Wait /^\$$/

Caption "1 · build: twelve paragraphs and one spec in, one .urna out"
Type "urna build --spec demo/starter/corpus.toml" 0.03
Key Enter
Wait /^\$$/
Sleep 2.4
Snap 01-build

Caption "2 · validate: every checksum and hash in the file"
Type "clear; urna validate demo/starter/out/quickstart.urna" 0.03
Key Enter
Wait /^\$$/
Poster
Sleep 2.4
Snap 02-validate
Key ctrl+d
