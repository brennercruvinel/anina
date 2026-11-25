use super::*;

fn red() -> Redactor {
    let mut r = Redactor::default();
    r.literal_as("home path", "/Users/alice", "~");
    r.word_as("user name", "alice", "user");
    r.add("config rule", r"/Applications/Secret[^ ]*", None).unwrap();
    r.add("config rule", r"Dia\.app", Some("Browser.app")).unwrap();
    r
}

#[test]
fn a_mask_keeps_the_width_and_the_columns_after_it() {
    let mut s = anina_screen::escape::parse("/Applications/Secret.app/x      |end\r\n", 40, 2);
    red().apply(&mut s);
    assert_eq!(s.row_text(0).trim_end(), "••••••••••••••••••••••••••      |end");
}

#[test]
fn a_replacement_slides_the_text_but_not_the_next_column() {
    let mut s = anina_screen::escape::parse("/Users/alice/notes   |end\r\n", 40, 2);
    red().apply(&mut s);
    let row = s.row_text(0);
    assert!(row.starts_with("~/notes"), "{row:?}");
    assert_eq!(row.find("|end"), Some(21), "{row:?}");
}

#[test]
fn a_row_that_fills_the_width_keeps_the_next_row_in_place() {
    // 20 columns: the first row is full, the next is output that only looks like its tail
    let mut s = anina_screen::escape::parse("ab /Users/alice/xyzw\r\nhello\r\n", 20, 3);
    red().apply(&mut s);
    assert_eq!(s.row_text(0).trim_end(), "ab ~/xyzw");
    assert_eq!(s.row_text(1).trim_end(), "hello");
}

#[test]
fn a_path_cut_by_the_wrap_is_still_hidden() {
    let mut s = anina_screen::escape::parse("cd /Users/ali\r\nce/notes\r\n", 13, 3);
    red().apply(&mut s);
    assert_eq!(s.row_text(0).trim_end(), "cd ~/notes");
}

#[test]
fn a_per_user_temp_folder_is_hidden_and_found_but_tmp_is_not() {
    assert!(user_tmp_from(Some("/tmp/".into())).is_empty());
    assert!(user_tmp_from(Some("/var/tmp".into())).is_empty());
    assert!(user_tmp_from(None).is_empty());
    assert_eq!(user_tmp_from(Some("/nonexistent/mj/abc/T/".into())), ["/nonexistent/mj/abc/T"]);
    let mut r = Redactor::default();
    r.literal_as("temp path", "/private/var/folders/mj/abc/T", "$TMPDIR");
    r.literal_as("temp path", "/var/folders/mj/abc/T", "$TMPDIR");
    let line = "anina: svg -> /var/folders/mj/abc/T//tmp.X/eof.svg and /private/var/folders/mj/abc/T/y";
    assert_eq!(r.text(line), "anina: svg -> $TMPDIR//tmp.X/eof.svg and $TMPDIR/y");
    assert_eq!(r.leaks(line).len(), 2);
    assert!(r.leaks("/tmp/tmp.X/eof.svg").is_empty());
}

#[test]
fn a_user_name_goes_only_as_a_whole_word() {
    assert_eq!(red().text("alice ran malice"), "user ran malice");
}

#[test]
fn text_and_json_are_redacted() {
    let r = red();
    assert_eq!(r.text("open /Users/alice/Dia.app"), "open ~/Browser.app");
    let mut v = serde_json::json!({"tape": "/Users/alice/t.tape", "n": 1, "list": ["alice"]});
    r.json(&mut v);
    assert_eq!(v, serde_json::json!({"tape": "~/t.tape", "n": 1, "list": ["user"]}));
}

#[test]
fn a_leak_is_reported_once_per_place() {
    let r = red();
    let leaks = r.leaks("pid 1 alice /Users/alice/x");
    let labels: Vec<&str> = leaks.iter().map(|(l, _)| *l).collect();
    assert_eq!(labels, ["user name", "home path"]);
    assert!(r.leaks("pid 1 user ~/x").is_empty());
}

#[test]
fn uncounted_rules_are_not_leaks() {
    let mut r = Redactor::default();
    r.literal("/tmp/sandbox", "/sandbox");
    assert!(r.leaks("/tmp/sandbox/work").is_empty());
}

#[test]
fn a_replacement_its_pattern_matches_is_refused() {
    assert!(Redactor::default().push("a+", "aa").is_err());
}

#[test]
fn the_config_adds_replace_and_mask_rules() {
    let dir = std::env::temp_dir().join(format!("anina-cfg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("anina.toml");
    std::fs::write(&p, "[redact]\nreplace = { 'Zed\\.app' = \"Editor.app\" }\nmask = ['tok-[0-9]+']\n").unwrap();
    let mut r = Redactor::default();
    r.load_config(&p).unwrap();
    assert_eq!(r.text("Zed.app tok-1234"), "Editor.app ••••••••");
    std::fs::write(&p, "[redact]\nmasks = ['x']\n").unwrap();
    assert!(Redactor::default().load_config(&p).is_err(), "a typo in a key is an error");
    let _ = std::fs::remove_file(&p);
}
