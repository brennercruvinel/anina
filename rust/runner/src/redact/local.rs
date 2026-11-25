//! What is this machine's own and goes out of everything written: the user,
//! the host, the git identity, the per-user temp folder.

#[derive(Clone, Copy)]
pub(super) enum Who {
    User,
    Host,
    Name,
    Email,
}

/// `TMPDIR` when it is this user's own folder (macOS: `/var/folders/<id>/T`),
/// as written and as the system resolves it (`/private/var/...`). The shared
/// `/tmp` and `/var/tmp` name no one.
pub fn user_tmp() -> Vec<String> {
    user_tmp_from(std::env::var("TMPDIR").ok())
}

pub(super) fn user_tmp_from(t: Option<String>) -> Vec<String> {
    let Some(t) = t else { return Vec::new() };
    let t = t.trim_end_matches('/').to_string();
    if t.is_empty() || t == "/tmp" || t == "/var/tmp" {
        return Vec::new();
    }
    let real = std::fs::canonicalize(&t).ok().map(|p| p.display().to_string()).filter(|r| *r != t);
    // the longer form first, so `/private/var/...` is not left as `/private$TMPDIR`
    real.into_iter().chain([t]).collect()
}

/// Who and where this is: user names, host names, git identity.
pub(super) fn identity() -> Vec<(Who, String)> {
    let mut out = Vec::new();
    let mut add = |w: Who, v: String| {
        let v = v.trim().to_string();
        if !v.is_empty() && !out.iter().any(|(_, x): &(Who, String)| *x == v) {
            out.push((w, v));
        }
    };
    let cmd = |prog: &str, args: &[&str]| {
        std::process::Command::new(prog)
            .args(args)
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    // longest first, so `Brenner Cruvinel` goes before a user name inside it
    add(Who::Name, cmd("git", &["config", "--global", "user.name"]));
    add(Who::Email, cmd("git", &["config", "--global", "user.email"]));
    let host = hostname();
    if let Some(short) = host.split('.').next().filter(|s| *s != host) {
        let short = short.to_string();
        add(Who::Host, host);
        add(Who::Host, short);
    } else {
        add(Who::Host, host);
    }
    for k in ["USER", "LOGNAME"] {
        add(Who::User, std::env::var(k).unwrap_or_default());
    }
    add(Who::User, cmd("id", &["-un"]));
    out
}

pub(super) fn hostname() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is valid for its length and gethostname NUL-terminates within it
    let ok = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) } == 0;
    if !ok {
        return String::new();
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}
