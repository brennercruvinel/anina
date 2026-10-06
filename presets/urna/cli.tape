# urna's command line on the starter corpus: help, the corpus checks,
# search, ask, retrieve, doctor. One screenshot per verb.
Set Cols 110
Set Rows 40
Set FPS 8
Set WaitTimeout 60
Set EndHold 2
Set CleanEnv on
Set Env HOME=$URNA_HOME PATH=$URNA_BIN:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin LANG=en_US.UTF-8 PS1='$ '
Set Cwd $URNA_CORPUS_DIR
Exec test -f "$URNA_CORPUS_DIR/quickstart.urna" || { echo "no quickstart.urna in $URNA_CORPUS_DIR: set URNA_REPO (to build it) or URNA_CORPUS_DIR" >&2; exit 1; }

Spawn sh
Wait /^\$$/

Caption "1 · urna --help: every verb"
Type "clear; urna --help" 0.03
Key Enter
Wait /^\$$/
Snap 01-help

Caption "2 · inspect: what is inside the file"
Type "clear; urna inspect quickstart.urna" 0.03
Key Enter
Wait /^\$$/
Snap 02-inspect

Caption "3 · validate: every hash checked"
Type "clear; urna validate quickstart.urna" 0.03
Key Enter
Wait /^\$$/
Snap 03-validate

Caption "4 · stats"
Type "clear; urna stats quickstart.urna" 0.03
Key Enter
Wait /^\$$/
Snap 04-stats

Caption "5 · search-text: embedded offline, ranked"
Type "clear; urna search-text quickstart.urna 'how do citations work' -k 3" 0.03
Key Enter
Wait /^\$$/
Snap 05-search-text

Caption "6 · ask: a cited answer"
Type "clear; urna ask quickstart.urna 'can I use this offline' -k 3" 0.03
Key Enter
Wait /^\$$/
Snap 06-ask

Caption "7 · retrieve: the same hits as JSON lines, for agents"
Type "clear; urna retrieve quickstart.urna 'how do citations work' -k 2 --format jsonl" 0.03
Key Enter
Wait /^\$$/
Snap 07-retrieve

Caption "8 · doctor: the install, checked"
Type "clear; urna doctor" 0.03
Key Enter
Wait /^\$$/
Snap 08-doctor
Key ctrl+d
