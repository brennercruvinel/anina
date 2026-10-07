# helix on a small Rust crate: open a file, search, change a word, pick
# another file, save and quit. The crate comes from this preset; the language
# server is turned off so the screen shows the editor, not its startup noise.
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
Wait "fn main"
Wait "NOR"
Sleep 1.6
Snap 01-open

Caption "2 · / searches: the match comes selected"
Key /
Sleep 0.3
Type "hello" 0.1
Sleep 0.4
Key Enter
Sleep 1.2
Snap 02-search

Caption "3 · c changes the selection, esc goes back to normal"
Key c
Wait "INS"
Sleep 0.4
Type "welcome" 0.1
Sleep 0.6
Key Esc
Wait "NOR"
Sleep 1
Poster
Snap 03-change

Caption "4 · space f: the file picker"
Key Space
Sleep 0.5
Key f
Wait "lib.rs"
Sleep 1.4
Snap 04-picker
Key Esc
Sleep 0.6

Caption "5 · :wq writes and quits"
Type ":wq" 0.12
Sleep 0.6
Key Enter
Sleep 0.8
# the proof: the change is on disk, not only on screen
Exec grep -q 'welcome, {name}' "$WORK/greeter/src/main.rs"
