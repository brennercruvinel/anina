use super::*;

/// The lines, with the cursor shown after the last one.
fn screen(lines: &[&str]) -> Screen {
    let mut s = anina_screen::escape::parse(&lines.join("\r\n"), 60, lines.len() as u16);
    s.cursor.visible = true;
    s
}

#[test]
fn a_scroll_is_seen_as_one() {
    let a = screen(&["title", "line 1", "line 2", "line 3", "line 4", "line 5", "status"]);
    let b = screen(&["title", "line 2", "line 3", "line 4", "line 5", "line 6", "status"]);
    assert!(scrolled(&a, &b));
    let c = screen(&["title", "other", "things", "on", "a", "new screen", "status"]);
    assert!(!scrolled(&a, &c));
}

fn sync(before: &Screen, after: &Screen, step: Option<&Out>) -> Vec<String> {
    let mut out = Vec::new();
    push_sync(&mut out, before, after, &Mask::new(before.cols, before.rows), 1.0, &Seen::default(), step);
    out
}

#[test]
fn a_line_that_scrolled_off_the_top_is_not_gone() {
    // review of #58, 1: a long command wraps on the last row and the screen scrolls by one
    let a = screen(&["$ ls -la", "a.txt", "b.txt", "c.txt", "d.txt", "$ "]);
    let typed = "echo one two three four five six seven eight nine ten eleven";
    let line = format!("$ {typed}");
    let (head, tail) = line.split_at(60);
    let b = screen(&["a.txt", "b.txt", "c.txt", "d.txt", head, tail]);
    let w = sync(&a, &b, Some(&Out::Type(typed.into())));
    assert!(w.iter().all(|l| !l.starts_with("Gone") && !l.starts_with("Idle")), "{w:?}");
    // the echo, found across the wrap
    assert_eq!(w, ["Wait eleven"]);
}

#[test]
fn a_wide_recording_reaches_fewer_rows_after_a_scroll() {
    // review of #58, 5: 190-column rows take four rows each at 60 columns
    let wide = |i: usize| format!("{i:02} {}", "x".repeat(187));
    let mk = |v: &[String]| {
        let mut s = anina_screen::escape::parse(&v.join("\r\n"), 200, v.len() as u16);
        s.cursor.visible = true;
        s
    };
    let mut rows: Vec<String> = (0..11).map(wide).collect();
    rows.push("$ make".into());
    let mut moved: Vec<String> = rows[5..].to_vec();
    moved.extend(["result is ready".to_string(), wide(97), wide(98), wide(99), "$ ".into()]);
    let (a, b) = (mk(&rows), mk(&moved));
    assert!(scrolled(&a, &b));
    // three wide rows sit between it and the cursor: 13 rows at 60 columns
    assert_eq!(sync(&a, &b, Some(&Out::Key("enter".into(), 1))), ["Idle 3"]);
    // two narrow ones do not
    moved[8..10].clone_from_slice(&["short one".to_string(), "short two".into()]);
    let b = mk(&moved);
    assert_eq!(sync(&a, &b, Some(&Out::Key("enter".into(), 1))), ["Wait 'result is ready'"]);
}

#[test]
fn a_word_cut_at_the_edge_is_no_wait() {
    // review of #58, 6: `comp` ends a full row, `lete` starts the next
    let mk = |t: &str| {
        let mut s = anina_screen::escape::parse(t, 60, 4);
        s.cursor.visible = true;
        s
    };
    let head = format!("{} comp", "x".repeat(55));
    let (a, b) = (mk("$ "), mk(&format!("{head}\r\nlete\r\n$ ")));
    assert!(b.row_full(0));
    let c = candidates(&a, &b, &Mask::new(60, 4), &Seen::default());
    assert!(c.iter().all(|(_, w)| !w.contains("comp") && !w.contains("lete")), "{c:?}");
    // text the replay still finds across the edge is not gone
    assert!(vanished(&mk("complete\r\n$ "), &b, &Mask::new(60, 4)).is_none());
}

#[test]
fn nothing_past_the_narrow_edge_is_waited_on() {
    // review of #70, 5: at 60 columns a TUI cuts what ends past column 56
    let pad = |l: &str, r: &str| format!("{l:<64}{r}");
    let mk = |rows: &[String]| {
        let mut s = anina_screen::escape::parse(&rows.join("\r\n"), 120, rows.len() as u16);
        s.cursor.visible = true;
        s
    };
    let a = mk(&[pad("  readme.md", "markdown notes"), pad("  src/main.rs", ""), "> ".into()]);
    let b = mk(&[pad("  readme.md", ""), pad("  src/main.rs", "now matching"), "> src".into()]);
    assert_eq!(sync(&a, &b, Some(&Out::Type("src".into()))), ["Idle 3"]);
    // a whole new row is new even when a longer row held its text
    let a = screen(&["$ echo next"]);
    let b = screen(&["$ echo next", "next", "$ "]);
    let a = anina_screen::escape::parse(&format!("{}\r\n\r\n", a.row_text(0)), 60, 3);
    assert_eq!(sync(&a, &b, Some(&Out::Key("enter".into(), 1))), ["Wait '/^next$/'"]);
}

#[test]
fn a_cursor_that_jumps_does_not_make_the_screen_its_echo() {
    // review of #70, 6: in less, `/` takes the cursor from the top to the last row
    let mut a = anina_screen::escape::parse("alpha\r\nbeta install\r\ngamma\r\n\r\n:\x1b[1;1H", 60, 5);
    a.cursor.visible = true;
    let b = screen(&["alpha", "beta install", "gamma", "pattern found", "/install"]);
    assert_eq!(a.cursor.y, 0);
    assert_eq!(sync(&a, &b, Some(&Out::Type("/install".into()))), ["Wait /install", "Wait 'pattern found'"]);
}

fn session(inputs: Vec<(f64, &str, Screen)>, last: Screen) -> Session {
    let (cols, rows) = (last.cols, last.rows);
    Session {
        cols,
        rows,
        argv: vec!["glow".into()],
        cwd: None,
        env: Vec::new(),
        clean_env: false,
        inputs: inputs
            .into_iter()
            .map(|(t, text, before)| Input { t, ev: Ev::Text(text.into()), before, quiet: Mask::new(cols, rows) })
            .collect(),
        resizes: Vec::new(),
        end: 3.0,
        last,
        last_quiet: Mask::new(cols, rows),
        volatile: Mask::new(cols, rows),
    }
}

#[test]
fn a_key_that_opens_a_field_is_a_step_of_its_own() {
    // review of #70, 7: `/` then `install` under 0.25 s apart was one Type
    let list = screen(&["  roadmap.md", "  install-guide.md", "  contributing.md", "", "  / filter"]);
    let bar =
        screen(&["  roadmap.md", "  install-guide.md", "  contributing.md", "enter open • esc cancel", "  Find: "]);
    let typed = |t: &str| screen(&["  install-guide.md", "", "", "enter open • esc cancel", &format!("  Find: {t}")]);
    let s = session(
        vec![(1.0, "/", list.clone()), (1.1, "i", bar.clone()), (1.15, "nstall", typed("i"))],
        typed("install"),
    );
    let steps: Vec<Out> = group(&s).into_iter().map(|g| g.0).collect();
    assert_eq!(steps, [Out::Type("/".into()), Out::Type("install".into())]);
    let tape = write(&s);
    assert!(tape.contains("Type '/'\nWait 'enter open • esc cancel'\nType"), "{tape}");
    // a path typed at a prompt stays one Type: only the cursor row changed
    let p = |t: &str| screen(&["$ ls", "a.txt", &format!("$ {t}")]);
    let s = session(vec![(1.0, "/", p("")), (1.1, "usr/bin", p("/"))], p("/usr/bin"));
    let steps: Vec<Out> = group(&s).into_iter().map(|g| g.0).collect();
    assert_eq!(steps, [Out::Type("/usr/bin".into())]);
}

#[test]
fn seen_keeps_each_line_once() {
    // review of #70, 10: the same screens over a long session
    let a = screen(&["  readme.md", "  src/main.rs", "> "]);
    let b = screen(&["  readme.md", "  1 match", "> src"]);
    let mut seen = Seen::default();
    for _ in 0..1000 {
        seen.add(&a);
        seen.add(&b);
    }
    assert_eq!(seen.lines.len(), 5);
    assert!(seen.holds("main.rs") && seen.holds("1 match") && !seen.holds("readme.md  "));
    assert!(seen.had_line("  src/main.rs") && !seen.had_line("src/main.rs"));
}

#[test]
fn a_whole_row_is_measured_in_columns() {
    // review of #97, 3: a wide character takes two columns
    let mk = |rows: &[&str]| {
        let mut s = anina_screen::escape::parse(&rows.join("\r\n"), 120, rows.len() as u16);
        s.cursor.visible = true;
        s
    };
    // a whole-row wait holds 24 characters at most, so a long row is never
    // one; an indented short one is: 51 chars over 57 columns, 48 over 58
    let emoji = format!("{}done 🚀🚀🚀🚀🚀🚀", " ".repeat(40));
    let cjk = format!("{}日本語のテキスト終わり", " ".repeat(38));
    for row in [emoji.as_str(), cjk.as_str()] {
        let w = sync(&mk(&["$ run", "", "$ "]), &mk(&["$ run", row, "$ "]), Some(&Out::Key("enter".into(), 1)));
        assert!(w.iter().all(|l| !l.contains("/^")), "{row}: {w:?}");
    }
}

#[test]
fn a_one_letter_filter_still_waits_for_the_filter() {
    // review of #58, 7: typing `e` in fzf hid every row holding an e
    let a = screen(&["  readme.md", "  src/main.rs", "  test.rs", "  3/3", "> "]);
    let b = screen(&["", "  readme.md", "  test.rs", "  2/3", "> e"]);
    assert_eq!(sync(&a, &b, Some(&Out::Type("e".into()))), ["Gone src/main.rs"]);
}

#[test]
fn a_scrolled_shell_waits_for_the_output_by_the_prompt() {
    let a = screen(&["$ ls", "a", "b", "c", "d", "e", "f", "$ cargo build"]);
    let b = screen(&["b", "c", "d", "e", "f", "$ cargo build", "    Finished `dev` profile in 3.2s", "$ "]);
    assert!(scrolled(&a, &b));
    let w = sync(&a, &b, Some(&Out::Key("enter".into(), 1)));
    assert_eq!(w, ["Wait 'Finished `dev` profile'"]);
}

#[test]
fn a_scroll_with_new_text_only_far_from_the_cursor_settles() {
    let rows: Vec<String> = (1..=20).map(|i| format!("row {i} of the log")).collect();
    let mut moved: Vec<String> = rows[3..].to_vec();
    moved.extend(["fresh tail text".to_string(), "more tail words".into(), "last tail line".into()]);
    let a = screen(&rows.iter().map(String::as_str).collect::<Vec<_>>());
    let mut b = anina_screen::escape::parse(&format!("{}\x1b[1;1H", moved.join("\r\n")), 60, 20);
    b.cursor.visible = true;
    assert!(scrolled(&a, &b));
    assert_eq!(sync(&a, &b, Some(&Out::Key("down".into(), 3))), ["Idle 3"]);
}

#[test]
fn after_a_filter_the_wait_is_what_it_took_away() {
    let a = screen(&["  README.md", "  src/main.rs", "  Cargo.toml", "  3/3", "> "]);
    let b = screen(&["", "", "  src/main.rs", "  1/3", "> src"]);
    assert_eq!(sync(&a, &b, Some(&Out::Type("src".into()))), ["Gone Cargo.toml"]);
    let a = screen(&["  README.md", "  src/main.rs", "  docs/install-guide.md", "  3/3", "> "]);
    let b = screen(&["", "", "  docs/install-guide.md", "  1/3", "> install guide"]);
    assert_eq!(sync(&a, &b, Some(&Out::Type("install guide".into()))), ["Wait 'install guide'", "Gone src/main.rs"]);
}

#[test]
fn a_filter_bar_is_not_enough_the_rows_must_go() {
    let a = screen(&["  roadmap.md", "  install-guide.md", "  contributing.md", "", "  ↑/↓ choose"]);
    let b = screen(&["  install-guide.md", "", "", "enter open • esc cancel", "  Find: install"]);
    let w = sync(&a, &b, Some(&Out::Type("/install".into())));
    assert_eq!(w, ["Wait 'enter open • esc cancel'", "Gone contributing.md"]);
}

#[test]
fn typing_at_a_prompt_waits_for_the_echo_alone() {
    let a = screen(&["$ ls", "Cargo.toml", "$ "]);
    let b = screen(&["$ ls", "Cargo.toml", "$ cargo build --release"]);
    assert_eq!(sync(&a, &b, Some(&Out::Type("cargo build --release".into()))), ["Wait --release"]);
}

#[test]
fn after_typing_the_wait_is_the_end_of_the_text() {
    let a = screen(&["> select x", "", ""]);
    let b = screen(&["> ct x from sales order by downloads desc", "", ""]);
    let tail = typed_tail("select x from sales order by downloads desc", &a, &b);
    assert_eq!(tail.as_deref(), Some("downloads desc"));
}

#[test]
fn a_padded_row_is_not_a_whole_row_wait() {
    let a = screen(&["", "", ""]);
    let b = screen(&["close      write      quit", "", ""]);
    let c = candidates(&a, &b, &Mask::new(60, 3), &Seen::default());
    assert!(c.iter().all(|(_, w)| !w.starts_with("'/^")), "{c:?}");
}
