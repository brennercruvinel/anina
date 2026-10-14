# urna tui on the starter corpus: open it, ask it, walk the cited hits,
# check the install, quit. Reads the HOME urna setup installed into. After
# 0.5.4 the home screen no longer lists the folder's files, so the tape
# opens the file by name.
Set Cols 108
Set Rows 30
Set FPS 10
Set WaitTimeout 40
Set EndHold 3
Set Env HOME=$URNA_HOME PATH=$URNA_BIN:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin
Set Cwd $URNA_CORPUS_DIR
Exec test -f "$URNA_CORPUS_DIR/quickstart.urna" || { echo "no quickstart.urna in $URNA_CORPUS_DIR: set URNA_REPO (to build it) or URNA_CORPUS_DIR" >&2; exit 1; }

Spawn "$URNA_BIN/urna" tui quickstart.urna
Caption "1 · urna tui quickstart.urna: the file, opened and checked"
Wait "sections"
Gone "opening "
Sleep 2.4
Snap 01-corpus

Caption "2 · ask: text in, cited answer out"
Key a
Sleep 0.6
Type "how do citations work" 0.07
Sleep 0.4
Key Enter
Wait "hits for"
Sleep 0.8
Poster
Snap 02-ask
Sleep 1.4

Caption "3 · each hit: the exact rerank score, the stored text, the citation"
Key Down
Sleep 1.4
Key Down
Sleep 1.6
Snap 03-hit

Caption "4 · health: the doctor, live"
Key Tab
Wait "every check passes"
Sleep 2.4
Snap 04-health

Caption "5 · quit: ctrl+q, from any tab"
Key C-q
Sleep 1
