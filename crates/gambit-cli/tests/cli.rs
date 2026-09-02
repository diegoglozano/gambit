use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run_with_stdin(arguments: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run gambit");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(input)
        .expect("write PGN");
    child.wait_with_output().expect("wait for gambit")
}

#[test]
fn help_describes_doctor() {
    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .arg("--help")
        .output()
        .expect("run gambit");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("gambit doctor"));
    assert!(stdout.contains("--format <human|json>"));
}

#[test]
fn validates_pgn_from_standard_input() {
    let output = run_with_stdin(
        &["doctor", "-"],
        b"[Event \"Example\"]\n\n1. e4 e5 2. Nf3 *\n",
    );

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("games: 1"));
    assert!(stdout.contains("moves: 3"));
}

#[test]
fn compatibility_form_still_validates() {
    let output = run_with_stdin(&["-"], b"1. e4 *\n");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("valid: stdin"));
}

#[test]
fn emits_a_json_success_report() {
    let output = run_with_stdin(&["doctor", "--format", "json", "-"], b"1. e4 e5 *\n");
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["status"], "valid");
    assert_eq!(report["source"], "stdin");
    assert_eq!(report["mode"], "semantic");
    assert_eq!(report["outcome_required"], true);
    assert_eq!(report["games"], 1);
    assert_eq!(report["moves"], 2);
    assert!(report["diagnostic"].is_null());
}

#[test]
fn diagnoses_illegal_moves_as_json() {
    let output = run_with_stdin(
        &["doctor", "--format=json", "-"],
        b"[Event \"Example\"]\n\n1. e5 *\n",
    );
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "invalid");
    assert_eq!(report["diagnostic"]["category"], "illegal_move");
    assert_eq!(report["diagnostic"]["game"], 1);
    assert_eq!(report["diagnostic"]["ply"], 1);
    assert_eq!(report["diagnostic"]["context"], "e5");
}

#[test]
fn diagnoses_invalid_fen() {
    let output = run_with_stdin(
        &["doctor", "--format=json", "-"],
        b"[FEN \"not a FEN\"]\n\n*\n",
    );
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostic"]["category"], "invalid_fen");
}

#[test]
fn diagnoses_pgn_syntax_errors() {
    let output = run_with_stdin(
        &["doctor", "--format=json", "-"],
        b"[Event \"Example\"]\n\n1. e4 ) *\n",
    );
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostic"]["category"], "syntax");
}

#[test]
fn supports_syntax_only_and_lenient_modes() {
    let output = run_with_stdin(&["doctor", "--syntax-only", "--lenient", "-"], b"1. e5\n");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("mode: syntax"));
    assert!(stdout.contains("moves: 1"));
}

#[test]
fn quiet_mode_suppresses_valid_reports() {
    let output = run_with_stdin(&["doctor", "--quiet", "-"], b"1. e4 *\n");
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn missing_input_uses_the_usage_exit_code() {
    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .arg("doctor")
        .output()
        .expect("run gambit");
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn missing_file_uses_the_input_exit_code() {
    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["doctor", "this-file-does-not-exist.pgn"])
        .output()
        .expect("run gambit");
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to open"));
}

#[test]
fn input_errors_respect_json_output() {
    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["doctor", "--format=json", "this-file-does-not-exist.pgn"])
        .output()
        .expect("run gambit");
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "error");
    assert_eq!(report["diagnostic"]["category"], "input");
}
