# The smallest journey: a shell in an empty sandbox. Paths show as `~` and
# `/sandbox` because anina redacts them before anything is written.
Set Cols 84
Set Rows 18
Set FPS 12
Set EndHold 2.5
Set Env PS1='$ '
Exec mkdir -p "$WORK/notes" && printf 'hello from anina\n' > "$WORK/notes/hello.txt"

Spawn sh
Wait '/\$$/'
Caption "1 · where the sandbox puts you"
Type "pwd; echo ~"
Key Enter
Wait "/sandbox/work"
Sleep 0.8
Snap 01-where

Caption "2 · files"
Type "ls notes && cat notes/hello.txt"
Key Enter
Wait "hello from anina"
Sleep 0.8
Snap 02-files

Caption "3 · colors"
Type "printf '\033[1;32mgreen\033[0m \033[38;2;255;120;0mtruecolor\033[0m \033[4munderline\033[0m\n'"
Key Enter
Wait "truecolor"
Sleep 0.8
Snap 03-colors
Poster

Caption "4 · a clean environment"
Type "env | cut -d= -f1 | sort | tr '\n' ' '; echo"
Key Enter
Wait "XDG_STATE_HOME"
Sleep 1.2
Snap 04-env

Caption ""
Type "exit"
Key Enter
Sleep 0.4
