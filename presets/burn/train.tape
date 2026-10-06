# Burn trains MNIST on the CPU while its dashboard draws the metrics live.
# The run is cut short through the dashboard's own quit popup (k: kill), so
# the journey ends in seconds instead of five epochs.
#
# Seen while building this: stopping with `s` moves on to evaluation, which
# downloads the test set and prints that progress over the dashboard.
Set Cols 120
Set Rows 34
Set FPS 6              # the plots redraw constantly: fewer samples
Set WaitTimeout 180    # the first run downloads MNIST before the dashboard
Set EndHold 2.5

Spawn "$BURN_DIR/target/release/examples/mnist"
Caption "1 · burn trains MNIST on the CPU; the dashboard comes up"
Wait "Progress"
Wait '/Iteration\s*:\s*[1-9]/'
Sleep 2
Snap 01-dashboard

Caption "2 · accuracy climbs, batch by batch"
Wait '/Iteration\s*:\s*[4-9][0-9]/'
Sleep 1
Poster
Snap 02-accuracy

Caption "3 · → the loss plot"
Key Right
Sleep 2.5
Snap 03-loss

Caption "4 · ↓ recent history instead of the full one"
Key Down
Sleep 2.5
Snap 04-recent

Caption "5 · → the learning rate"
Key Right
Sleep 2.5
Snap 05-learning-rate

Caption "6 · q: stop, kill or cancel"
Key q
Wait "Kill the program"
Sleep 1.5
Snap 06-quit-popup

Caption "7 · c: cancel, the training goes on"
Key c
Gone "Kill the program"
Sleep 2

Caption "8 · q, k: kill it; the terminal comes back clean"
Key q
Wait "Kill the program"
Sleep 0.8
Key k
Gone "Progress"
Sleep 1
Snap 07-killed
