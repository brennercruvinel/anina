# harlequin on a CSV, offline: DuckDB reads the file straight from the query.
# The CSV comes from this preset, copied into the sandbox.
Set Cols 110
Set Rows 30
Set FPS 8
Set WaitTimeout 40
Set EndHold 2.5
Set Cwd $WORK
Exec cp "$PRESET_DIR/sales.csv" "$WORK/sales.csv"

Spawn harlequin
Caption "1 · harlequin: the catalog, the editor, the results"
Wait "Query Editor"
Wait "Run Query"
Sleep 1.6
Snap 01-open

Caption "2 · a query over sales.csv, typed in the editor"
Type "select program, sum(downloads) as downloads, max(stars) as stars from 'sales.csv' group by program order by downloads desc" 0.03
Sleep 0.4
Key Esc
Sleep 0.6
Snap 02-typed

Caption "3 · f4 formats it"
Key F4
Sleep 1.4
Snap 03-format

Caption "4 · ctrl+j runs it: three programs, ranked"
Key C-j
Wait "3 Records"
Wait "executed successfully"
# the proof: helix's downloads summed from the CSV (9,120 + 8,790 + 9,301)
Wait "27,211"
Sleep 1.8
Poster
Snap 04-results

Caption "5 · quit: ctrl+q"
Key C-q
Sleep 1
