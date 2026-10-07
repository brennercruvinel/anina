# glow on a folder of notes: the list, one note read top to bottom, back to
# the list, quit. The notes come from this preset, copied into the sandbox.
Set Cols 96
Set Rows 26
Set FPS 10
Set WaitTimeout 20
Set EndHold 2.5
Set Cwd $WORK
Exec cp -R "$PRESET_DIR/notes" "$WORK/notes"

Spawn glow notes
Caption "1 · glow: every markdown file in the folder"
Wait "release"
Sleep 1.6
Snap 01-list

Caption "2 · walk the list, then filter it: / and a few letters"
Key Down
Sleep 0.6
Key Down
Sleep 0.8
Key /
Sleep 0.4
Type "rele" 0.12
Sleep 1
Snap 02-filter
Key Enter
Sleep 0.6

Caption "3 · read it: headings, a table, code, a quote"
Key Enter
Wait "Release checklist"
Sleep 1.4
Poster
Snap 03-read

Caption "4 · scroll"
Key Down*6
Sleep 1.4
Snap 04-scroll

Caption "5 · back to the list, then quit"
Key Esc
Wait "ideas"
Sleep 1
Key q
Sleep 0.6
