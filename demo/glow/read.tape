# glow on a folder of notes: the list, a filter, one note read top to bottom,
# back to the list, quit. The notes come from this preset, copied into the
# sandbox. Steps and waits written by `anina init`; captions and snaps added.
Set Cols 96
Set Rows 26
Set FPS 10
Set WaitTimeout 20
Set EndHold 2.5
Set Cwd $WORK
Exec cp -R "$PRESET_DIR/notes" "$WORK/notes"

Spawn glow notes
Caption "1 · glow: every markdown file in the folder"
Wait '/ find • r refresh • e'
Sleep 1.2
Snap 01-list

Caption "2 · walk the list, then filter it: / and a few letters"
Key down
Wait '/^ │ meeting\.md$/'
Key down
Wait '/^ │ release\.md$/'
Type '/rele'
Wait 'enter open • esc cancel'
Sleep 0.8
Snap 02-filter

Caption "3 · enter opens the only match: headings, a table, code, a quote"
Key enter
Wait 'Everything that has to'
Sleep 1.4
Poster
Snap 03-read

Caption "4 · scroll"
Key down
Idle 3
Key down
Idle 3
Key down
Idle 3
Key down*3
Sleep 1.4
Snap 04-scroll

Caption "5 · back to the list, then quit"
Key esc
Wait '/ find • r refresh • e'
Sleep 0.8
Type 'q'
Sleep 0.6
