# harlequin on a CSV, offline: DuckDB reads the file straight from the query.
# The CSV comes from this preset, copied into the sandbox. Steps and waits
# written by `anina init`; captions, snaps and the proof added. The success
# toast is not waited on: harlequin hides it in narrow terminals.
Set Cols 110
Set Rows 30
Set FPS 8
Set WaitTimeout 40
Set EndHold 2.5
Set Cwd $WORK
Exec cp "$PRESET_DIR/sales.csv" "$WORK/sales.csv"

Spawn harlequin
Caption "1 · harlequin: the catalog, the editor, the results"
Wait 'Query Results'
Sleep 1.6
Snap 01-open

Caption "2 · a query over sales.csv, typed in the editor"
Type@30ms 'select program, sum(downloads) as downloads, max(stars) as stars from ' "'" 'sales.csv' "'" ' group by program order by downloads desc'
Wait 'downloads desc'
Key esc
Sleep 0.6
Snap 02-typed

Caption "3 · f4 formats it"
Key f4
# Idle, not a text: at 80 columns and below, harlequin keeps the editor scrolled to
# the right after formatting, so the formatted query is out of view
Idle 3
Sleep 1
Snap 03-format

Caption "4 · ctrl+j runs it: three programs, ranked"
Key ctrl+j
Wait 'downloads ###'
Wait "3 Records"
# the proof: helix's downloads summed from the CSV (9,120 + 8,790 + 9,301)
Wait "27,211"
Sleep 1.8
Poster
Snap 04-results

Caption "5 · quit: ctrl+q"
Key ctrl+q
Sleep 1
