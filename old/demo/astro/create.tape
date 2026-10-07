# create-astro in an empty sandbox: the folder, a template, no dependency
# install (keeps it fast), a git repository. npx fetches the wizard the first
# time, so the first wait is long.
Set Cols 90
Set Rows 28
Set FPS 10
Set WaitTimeout 120
Set EndHold 3
Set Env npm_config_update_notifier=false npm_config_fund=false npm_config_audit=false

Spawn npx --yes create-astro@latest
Caption "1 · npm create astro: the wizard starts"
Wait "Where should we create"
Sleep 1.2
Snap 01-folder

Caption "2 · the folder for the new site"
Type "./my-site" 0.08
Sleep 0.5
Key Enter
Wait "How would you like to start"
Sleep 1
Snap 02-template

Caption "3 · a template: the blog"
Key Down
Sleep 0.8
Snap 03-template-blog
Key Enter
Wait "Install dependencies"
Sleep 1

Caption "4 · dependencies: later (no npm install in the demo)"
Key Right
Sleep 1
Snap 04-deps-no
Key Enter
Wait "Initialize a new git repository"
Sleep 1

Caption "5 · a git repository: yes"
Snap 05-git
Key Enter
Wait "Liftoff confirmed"
Wait "Good luck out there"
Sleep 0.6
Caption "6 · liftoff: the project is ready"
Poster
Sleep 2.5
Snap 06-liftoff
