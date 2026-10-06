# urna tui on the starter corpus: open it, ask it, walk the cited hits,
# check the install, quit. Reads the HOME urna setup installed into.
Set Cols 108
Set Rows 30
Set FPS 10
Set WaitTimeout 40
Set EndHold 3
Set Env HOME=$URNA_HOME PATH=$URNA_BIN:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin
Set Cwd $URNA_CORPUS_DIR
Exec test -f "$URNA_CORPUS_DIR/quickstart.urna" || { echo "no quickstart.urna in $URNA_CORPUS_DIR: set URNA_REPO (to build it) or URNA_CORPUS_DIR" >&2; exit 1; }

Spawn "$URNA_BIN/urna" tui
Caption "1 · urna tui: the explorer"
Set FPS 3              # the home screen rains too
Wait "open another file"
Sleep 2.2
Snap 01-home

Caption "2 · open: 1 opens the quickstart.urna found in the folder"
Set FPS 10
Key 1
Wait "sections"
Gone "opening "
Sleep 2.4
Snap 02-corpus

Caption "3 · ask: text in, cited answer out"
Key a
Sleep 0.6
Type "how do citations work" 0.07
Sleep 0.4
Key Enter
Wait "hits for"
Sleep 0.8
Poster
Snap 03-ask
Sleep 1.4

Caption "4 · each hit: the exact rerank score, the stored text, the citation"
Key Down
Sleep 1.4
Key Down
Sleep 1.6
Snap 04-hit

Caption "5 · health: the doctor, live"
Key Tab
Wait "every check passes"
Sleep 2.4
Snap 05-health

Caption "6 · quit: ctrl+q, from any tab"
Key C-q
Sleep 1
