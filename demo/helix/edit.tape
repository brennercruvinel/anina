# helix on a small Rust crate: open a file, search, change a word, pick
# another file, save and quit. The crate comes from this preset; the language
# server is turned off so the screen shows the editor, not its startup noise.
# Steps and waits written by `anina init`; captions, snaps and the proof added.
Set Cols 100
Set Rows 26
Set FPS 10
Set WaitTimeout 20
Set EndHold 2.5
Set Cwd $WORK/greeter
Exec cp -R "$PRESET_DIR/greeter" "$WORK/greeter"
Exec mkdir -p "$HOME_DIR/.config/helix" && printf '[[language]]\nname = "rust"\nlanguage-servers = []\n' > "$HOME_DIR/.config/helix/languages.toml"

Spawn hx src/main.rs
Caption "1 · helix: a Rust file, colored by tree-sitter"
Wait 'let line = greet(&name)'
Sleep 1.4
Snap 01-open

Caption "2 · / searches: the match comes selected"
Type '/hello' 0.1
Wait search:hello
Key enter
Sleep 1.2
Snap 02-search

Caption "3 · c changes the selection, esc goes back to normal"
Type 'cwelcome' 0.1
Wait 'format!("welcome'
Key esc
Sleep 1
Poster
Snap 03-change

Caption "4 · space f: the file picker"
Type ' '
Wait Window
Type 'f'
# init only sleeps here: the picker draws itself in steps, so its cells
# count as changing; this tape waits on a file name it lists
Wait "lib.rs"
Sleep 1.4
Snap 04-picker
Key esc
Sleep 0.6

Caption "5 · :wq writes and quits"
Type ':wq' 0.12
Wait 'Write changes to disk'
Key enter
Sleep 0.8
# the proof: the change is on disk, not only on screen
Exec grep -q 'welcome, {name}' "$WORK/greeter/src/main.rs"
