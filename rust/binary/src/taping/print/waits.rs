//! What a written tape waits for after each step: the text that is new to
//! the session, the end of what was typed, the rows a filter took away, and
//! how each is cut so that it holds in every terminal an audit tries (ADR-0015).

use super::*;

/// The longest text a wait may hold. `anina audit` replays the tape down to
/// 60x20, where a panel keeps some 30 columns: a longer wait would not fit.
pub(super) const MAX_WAIT: usize = 24;

/// Text that ends past this column at the recording size is cut off in the
/// narrowest terminal `anina audit` tries (60 columns, less a border): no
/// `Wait` or `Gone` is written for it.
pub(super) const NARROW_COLS: u16 = 56;

/// Text keys closer than this are typing, whatever a completion popup does
/// to the screen in between.
pub(super) const TYPING_GAP: f64 = 0.25;

/// Text never seen earlier in the session ranks above any text that was:
/// what a step brings up beats what a closing popup uncovers.
pub(super) const NEW_TO_SESSION: i64 = 100;

/// The column after the last one row `y` fills, in columns: a wide
/// character takes two (`row_text` gives it one char).
pub(super) fn row_end(s: &Screen, y: u16) -> u16 {
    s.row(y).iter().rposition(|c| c.is_spacer() || c.ch != " ").map_or(0, |x| x as u16 + 1)
}

/// Row `y` as the narrowest terminal cuts it: its cells before `NARROW_COLS`.
pub(super) fn row_cut(s: &Screen, y: u16) -> String {
    let cells = &s.row(y)[..NARROW_COLS.min(s.cols) as usize];
    let text: String = cells.iter().filter(|c| !c.is_spacer()).map(|c| c.ch.as_str()).collect();
    text.trim_end().to_string()
}

/// Does text in these columns of row `y` touch an edge the terminal wrapped
/// at? Then a narrower or wider terminal cuts it elsewhere (`comp` at the end
/// of a full row, `lete` starting the next).
pub(super) fn at_edge(s: &Screen, y: u16, cols: &[u16]) -> bool {
    let (Some(first), Some(last)) = (cols.first(), cols.last()) else { return false };
    (*last + 1 >= s.cols && s.row_full(y)) || (*first == 0 && y > 0 && s.row_full(y - 1))
}

/// Every distinct line of the screens so far. A step's text is news only
/// when no earlier line holds it (a wait never spans lines). Kept as lines,
/// not screens: a session shows the same lines step after step, and each is
/// kept once, so writing a tape doesn't grow with the square of its length.
#[derive(Debug, Default)]
pub struct Seen {
    pub(super) lines: std::collections::HashSet<String>,
}

impl Seen {
    pub fn add(&mut self, s: &Screen) {
        for l in s.text().lines().filter(|l| !l.trim().is_empty()) {
            if !self.lines.contains(l) {
                self.lines.insert(l.to_string());
            }
        }
    }

    /// Whether an earlier line holds `text`.
    pub(super) fn holds(&self, text: &str) -> bool {
        self.lines.iter().any(|l| l.contains(text))
    }

    /// Whether `row` was an earlier line, whole.
    pub(super) fn had_line(&self, row: &str) -> bool {
        self.lines.contains(row)
    }
}

/// The best text to wait for after a step: new since `before`, unique on
/// `after`, made of cells that do not change by themselves, preferably free
/// of digits (timings and counters change from run to run), and new to the
/// whole session: text that was already on an earlier screen (`seen`) and
/// only came back from under a popup says little about what this step did.
/// Returns the tape argument (a literal, or a `/^row$/` regex for a whole new
/// row).
pub fn distinctive(before: &Screen, after: &Screen, volatile: &Mask, seen: &Seen) -> Option<String> {
    candidates(before, after, volatile, seen).into_iter().next().filter(|c| c.0 > 0).map(|c| c.1)
}

/// Every text `distinctive` could pick, best first, with its score.
pub fn candidates(before: &Screen, after: &Screen, volatile: &Mask, seen: &Seen) -> Vec<(i64, String)> {
    let (shown_before, shown_after) = (before.views(), after.views());
    let after_text = after.text();
    let before_lines: std::collections::HashSet<String> = (0..before.rows).map(|y| before.row_text(y)).collect();
    // before's rows as the narrowest terminal cuts them
    let before_cut: std::collections::HashSet<String> = (0..before.rows).map(|y| row_cut(before, y)).collect();
    let mut found: Vec<(i64, String)> = Vec::new();
    let mut consider = |score: i64, arg: String| found.push((score, arg));
    for y in 0..after.rows {
        let row = after.row_text(y);
        let row_new = !before_lines.contains(&row);
        for seg in segments(after, y) {
            let chars: Vec<char> = seg.text.chars().collect();
            // the whole segment and its digit-free word runs
            let mut spans = vec![(0, chars.len())];
            let mut start = None;
            let mut i = 0;
            while i <= chars.len() {
                let word_end = i == chars.len() || chars[i] == ' ';
                if word_end {
                    // find the word that just ended
                    let ws = chars[..i].iter().rposition(|c| *c == ' ').map_or(0, |p| p + 1);
                    let has_digit = chars[ws..i].iter().any(|c| c.is_ascii_digit());
                    if has_digit {
                        if let Some(s) = start.take() {
                            spans.push((s, ws));
                        }
                    } else if start.is_none() && ws < i {
                        start = Some(ws);
                    }
                }
                i += 1;
            }
            if let Some(s) = start {
                spans.push((s, chars.len()));
            }
            for (a, b) in spans {
                let (ta, tb) = trim_span(&chars[a..b]);
                let (mut a, mut b) = (a + ta, a + tb);
                if b - a > MAX_WAIT {
                    // cut at a word boundary
                    let cut = chars[a..a + MAX_WAIT].iter().rposition(|c| *c == ' ').unwrap_or(MAX_WAIT);
                    b = a + cut;
                    let (ta, tb) = trim_span(&chars[a..b]);
                    (a, b) = (a + ta, a + tb);
                }
                if b <= a {
                    continue;
                }
                let text: String = chars[a..b].iter().collect();
                let alnum = text.chars().filter(|c| c.is_alphanumeric()).count();
                if alnum < 3 || text.contains('\'') && text.contains('"') {
                    continue;
                }
                // cut off in the narrowest terminal: never there, or there from the start
                if seg.cols[b - 1] >= NARROW_COLS
                    || seg.cols[a..b].iter().any(|x| volatile.get(*x, seg.y))
                    || at_edge(after, seg.y, &seg.cols[a..b])
                {
                    continue;
                }
                // what the replay's wait would see: never on before, once on after
                if shown_before.shows(&text) || shown_after.count(&text) != 1 {
                    continue;
                }
                let digits = text.chars().any(|c| c.is_ascii_digit());
                let mut score = alnum.min(40) as i64;
                score += if digits { -20 } else { 8 };
                if text.split_whitespace().any(looks_like_id) {
                    score -= 10;
                }
                if row_new {
                    score += 4;
                }
                // digits are counters and clocks ("32 minutes ago"): new, but not news
                if !digits && !seen.holds(&text) {
                    score += NEW_TO_SESSION;
                }
                consider(score, quote(&text));
            }
        }
        // a whole new row, matched as a line
        let alnum = row.chars().filter(|c| c.is_alphanumeric()).count();
        let unique_line = after_text.lines().filter(|l| *l == row).count() == 1;
        let stable = (0..after.cols).all(|x| !volatile.get(x, y) || after.cell(x, y).is_blank());
        let padded = row.trim().contains("  ") || row.trim().chars().count() > MAX_WAIT;
        let wrapped = after.row_full(y) || y > 0 && after.row_full(y - 1);
        // a row that only lost its right part (a column a narrow terminal never
        // shows) is already there, whole, in that terminal
        let narrow = row_end(after, y) <= NARROW_COLS && !before_cut.contains(&row);
        if row_new && alnum >= 2 && unique_line && stable && !padded && !wrapped && narrow {
            let digits = row.chars().any(|c| c.is_ascii_digit());
            let mut score = alnum.min(40) as i64 - 2 + if digits { -20 } else { 8 };
            if !digits && !seen.had_line(&row) {
                score += NEW_TO_SESSION;
            }
            consider(score, quote(&format!("/^{}$/", regex::escape(&row))));
        }
    }
    // best score first; on a tie the one higher on screen (found first)
    found.sort_by_key(|c| std::cmp::Reverse(c.0));
    found
}

/// The smallest terminal `anina audit` tries, where a written wait must
/// still be on screen.
pub(super) const SMALL: (u16, u16) = (60, 20);

/// `Wait` for text the step brought up. After a scroll, only text near the
/// cursor: what shows far from it depends on the terminal's height, while
/// the rows next to the cursor (a shell's last output, an editor's new line)
/// show in every size.
pub(super) fn appeared(before: &Screen, after: &Screen, mask: &Mask, seen: &Seen) -> Option<String> {
    let w = if scrolled(before, after) {
        let far = far_from_cursor(after)?;
        distinctive(before, after, &union(mask, &far), seen)
    } else {
        distinctive(before, after, mask, seen)
    };
    w.map(|w| format!("Wait {w}"))
}

/// The rows of `s` that may be out of view in the smallest terminal once
/// the screen scrolled: those past half its height from the cursor, above
/// or below, counted in rows as they wrap at its width (a 190-column row
/// takes four). None when the cursor is hidden: then it says nothing about
/// where the program draws.
pub(super) fn far_from_cursor(s: &Screen) -> Option<Mask> {
    if !s.cursor.visible {
        return None;
    }
    let height = |y: u16| (s.row_text(y).chars().count() as u16).div_ceil(SMALL.0).max(1);
    let budget = SMALL.1 / 2;
    let mut near = vec![false; s.rows as usize];
    let mut used = 0;
    for y in (0..=s.cursor.y.min(s.rows.saturating_sub(1))).rev() {
        used += height(y);
        if used > budget {
            break;
        }
        near[y as usize] = true;
    }
    used = 0;
    for y in s.cursor.y + 1..s.rows {
        used += height(y);
        if used > budget {
            break;
        }
        near[y as usize] = true;
    }
    let mut m = Mask::new(s.cols, s.rows);
    for y in (0..s.rows).filter(|y| !near[*y as usize]) {
        (0..s.cols).for_each(|x| m.set(x, y));
    }
    Some(m)
}

/// After typing, the steps that show the program answered: text it brought
/// up off the echo's rows and text it took away (`Gone`: a filter dropping
/// rows), both when there are both (a filter's bar shows with its first key,
/// its rows go only once it ran), else `Idle`. Empty when nothing but the
/// echo's rows changed. When the screen scrolled, the rows that left at the
/// top left with the scroll (a long line wrapped on the last row), and in a
/// taller terminal they stay: only new text counts then.
pub(super) fn reaction(typed: &str, before: &Screen, after: &Screen, volatile: &Mask, seen: &Seen) -> Vec<String> {
    if before.cols != after.cols || before.rows != after.rows {
        return Vec::new();
    }
    let mut mask = volatile.clone();
    for y in echo_rows(typed, before, after) {
        (0..after.cols).for_each(|x| mask.set(x, y));
    }
    if scrolled(before, after) {
        return appeared(before, after, &mask, seen).into_iter().collect();
    }
    let d = before.diff(after);
    let moved = (0..after.rows).any(|y| (0..after.cols).any(|x| d.get(x, y) && !mask.get(x, y)));
    if !moved {
        return Vec::new();
    }
    let mut out: Vec<String> = appeared(before, after, &mask, seen).into_iter().collect();
    out.extend(vanished(before, after, &mask).map(|g| format!("Gone {}", quote(&g))));
    if out.is_empty() {
        out.push("Idle 3".into());
    }
    out
}

/// The rows the echo of `typed` may be on. With the cursor shown: the row
/// typing ended on and the rows the text wrapped over above it. Where the
/// cursor was before says nothing: a key that opened a field moved it there
/// (`/` in less takes it from the top to the last row). With it hidden: rows that end with the typed text, the way
/// an input field does (`Find: install`); a row merely holding it says
/// nothing (typing `e` would hide every row with an e).
pub(super) fn echo_rows(typed: &str, before: &Screen, after: &Screen) -> Vec<u16> {
    let last = after.rows.saturating_sub(1);
    if before.cursor.visible && after.cursor.visible {
        let len = typed.chars().count() as u16;
        let up = len.saturating_sub(after.cursor.x).div_ceil(after.cols.max(1));
        let hi = after.cursor.y.min(last);
        return (hi.saturating_sub(up)..=hi).collect();
    }
    let text = typed.trim_start_matches(|c: char| !c.is_alphanumeric()).trim_end();
    if text.is_empty() {
        return Vec::new();
    }
    (0..after.rows).filter(|y| after.row_text(*y).ends_with(text)).collect()
}

/// Text on `before` that `after` no longer shows, the way the replay's
/// `Gone` reads it: unique, stable, short, digit-free, not cut at a wrap
/// edge, from the row nearest the cursor (rows far from it may not be on a
/// smaller screen to begin with).
pub(super) fn vanished(before: &Screen, after: &Screen, mask: &Mask) -> Option<String> {
    let (shown_before, shown_after) = (before.views(), after.views());
    let cy = after.cursor.y;
    let mut best: Option<(u16, String)> = None;
    for y in 0..before.rows {
        for seg in segments(before, y) {
            let chars: Vec<char> = seg.text.chars().collect();
            let (a, b) = trim_span(&chars);
            // past the narrow edge it may never be on screen, and a Gone of it passes at once
            if b <= a
                || seg.cols[b - 1] >= NARROW_COLS
                || seg.cols[a..b].iter().any(|x| mask.get(*x, y))
                || at_edge(before, y, &seg.cols[a..b])
            {
                continue;
            }
            let text: String = chars[a..b].iter().collect();
            let ok = text.chars().filter(|c| c.is_alphanumeric()).count() >= 3
                && text.chars().count() <= MAX_WAIT
                && !text.chars().any(|c| c.is_ascii_digit())
                && !(text.contains('\'') && text.contains('"'))
                && shown_before.count(&text) == 1
                && !shown_after.shows(&text);
            let dist = y.abs_diff(cy);
            if ok && best.as_ref().is_none_or(|(d, _)| dist < *d) {
                best = Some((dist, text));
            }
        }
    }
    best.map(|(_, t)| t)
}

/// The shortest end of `typed`, cut at a word, that shows once on `after`
/// and not on `before`: the text next to the cursor, on screen at any width.
/// Leading signs may be keys that opened the field (`/` to filter, `:`).
pub(super) fn typed_tail(typed: &str, before: &Screen, after: &Screen) -> Option<String> {
    let (shown_before, shown_after) = (before.views(), after.views());
    let first_word = typed.find(char::is_alphanumeric).into_iter();
    let mut starts: Vec<usize> = std::iter::once(0).chain(first_word).collect();
    starts.extend(typed.match_indices(' ').map(|(i, _)| i + 1));
    starts.sort_unstable();
    starts.dedup();
    let mut tails: Vec<&str> = starts.into_iter().map(|i| &typed[i..]).collect();
    tails.reverse(); // shortest first
    tails
        .into_iter()
        .map(str::trim)
        .find(|t| {
            t.chars().filter(|c| c.is_alphanumeric()).count() >= 6
                && t.chars().count() <= MAX_WAIT
                && shown_after.count(t) == 1
                && !shown_before.shows(t)
        })
        .map(String::from)
}

/// Did the screen move up or down, most of its rows the same, shifted?
pub(super) fn scrolled(before: &Screen, after: &Screen) -> bool {
    if before.rows != after.rows || before.cols != after.cols {
        return false;
    }
    let rows = after.rows as i32;
    let row = |s: &Screen, y: i32| s.row_text(y as u16);
    let filled: Vec<i32> = (0..rows).filter(|y| !row(after, *y).trim().is_empty()).collect();
    let same = |d: i32| {
        filled.iter().filter(|&&y| (0..rows).contains(&(y + d)) && row(after, y) == row(before, y + d)).count()
    };
    let still = same(0);
    let best = (1..rows / 2).flat_map(|d| [d, -d]).map(same).max().unwrap_or(0);
    best >= 3 && best * 2 >= filled.len() && best > still
}
