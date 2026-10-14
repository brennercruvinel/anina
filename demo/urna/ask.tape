# urna ask on the starter corpus: a question in, the passage and its
# urna:// citation out, then urna cite turns that citation back into the
# stored text and the hashes it is bound to. The binary is the checkout's
# target/release/urna, which finds its embedder in the checkout; URNA_PYTHON
# is the checkout's .venv, the interpreter urna setup would otherwise make.
Set Cols 100
Set Rows 26
Set FPS 8
Set WaitTimeout 60
Set EndHold 3
Set CleanEnv on
Set Env HOME=$URNA_HOME PATH=$URNA_REPO/target/release:$URNA_BIN:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin URNA_PYTHON=$URNA_REPO/.venv/bin/python LANG=en_US.UTF-8 PS1='$ '
Set Cwd $URNA_CORPUS_DIR
Exec test -f "$URNA_CORPUS_DIR/quickstart.urna" || { echo "no quickstart.urna in $URNA_CORPUS_DIR: set URNA_REPO (to build it) or URNA_CORPUS_DIR" >&2; exit 1; }

Spawn sh
Wait /^\$$/

Caption "1 · ask: a question in, the passage and its citation out"
Type "urna ask quickstart.urna 'how do citations work' -k 1" 0.03
Key Enter
Wait /^\$$/
Poster
Sleep 3
Snap 01-ask

Caption "2 · cite: the citation back to the stored text and its hashes"
Type "urna cite quickstart.urna urna://sha256:b7c81eb1731850692d1f50f410064414957abc54d10c795fd9c3306294044689/sha256:b5dfeb09a643f6f0afde3f361316dde3ea503ff86c525d297e6ba79b5781a4be" 0.008
Key Enter
Wait /^\$$/
Sleep 3
Snap 02-cite
Key ctrl+d
