//! A tiny greeter, edited live by anina.

use greeter::shout;
use std::env;

/// The line printed for `name`.
fn greet(name: &str) -> String {
    format!("hello, {name}")
}

fn main() {
    let name = env::args().nth(1).unwrap_or_else(|| "world".into());
    let loud = env::var("LOUD").is_ok();
    let line = greet(&name);
    println!("{}", if loud { shout(&line) } else { line });
}
