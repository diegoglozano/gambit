use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn help_describes_the_binary() {
    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .arg("--help")
        .output()
        .expect("run gambit");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Usage: gambit"));
}

#[test]
fn validates_pgn_from_standard_input() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("run gambit");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(b"[Event \"Example\"]\n\n1. e4 e5 2. Nf3 *\n")
        .expect("write PGN");
    let output = child.wait_with_output().expect("wait for gambit");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("games: 1"));
    assert!(stdout.contains("legal SAN moves: 3"));
}
