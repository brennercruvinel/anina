# urna setup on an empty machine: the HOME is the sandbox's, PATH has only
# the system and the released binary. Needs network (~30 MB).
Set Cols 108
Set Rows 30
Set FPS 10
Set WaitTimeout 60
Set EndHold 3
Set Env HOME=$URNA_HOME PATH=$URNA_BIN:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin TERM_PROGRAM=ghostty

Spawn "$URNA_BIN/urna" setup
Caption "1 · urna setup: the installer every channel ends in"
Set FPS 3              # the splash breathes and rains all the time: fewer samples
Wait "press enter to begin"
Sleep 2
Snap 01-splash

Caption "2 · scan: what the machine already has, read only"
Set FPS 4
Key Enter
Wait "steps ─"
Set FPS 10
Sleep 0.6
Snap 02-plan

Caption "3 · plan: every step, what it downloads and where it goes"
Key Down
When "[ ] python env" Key Space
Sleep 2.4
Snap 03-plan-python

Caption "4 · install: the payload verified by sha256, the venv made by uv"
Key Enter
Speed 2.5
Wait "enter finish" 300
Speed 1
Set FPS 3
Caption "5 · verify: the doctor checks pass, offline"
Sleep 2.8
Snap 04-verify
Poster
Key Enter
Sleep 1
