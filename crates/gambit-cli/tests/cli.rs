use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

struct TestFile(PathBuf);

impl TestFile {
    fn new(suffix: &str, contents: &[u8]) -> Self {
        let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "gambit-cli-test-{}-{sequence}.{suffix}",
            std::process::id()
        ));
        fs::write(&path, contents).expect("write temporary PGN");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("gambit-cli-test-{}-{sequence}", std::process::id()));
        fs::create_dir(&path).expect("create temporary directory");
        Self(path)
    }

    fn write(&self, relative: impl AsRef<Path>, contents: &[u8]) -> PathBuf {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create temporary subdirectory");
        }
        fs::write(&path, contents).expect("write temporary file");
        path
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl Drop for TestFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn run_with_stdin(arguments: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run gambit");
    let write_result = child.stdin.take().expect("piped stdin").write_all(input);
    if let Err(error) = write_result {
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::BrokenPipe,
            "write PGN: {error}"
        );
    }
    child.wait_with_output().expect("wait for gambit")
}

#[test]
fn help_describes_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .arg("--help")
        .output()
        .expect("run gambit");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("gambit doctor"));
    assert!(stdout.contains("gambit stats"));
    assert!(stdout.contains("gambit index"));
    assert!(stdout.contains("gambit query"));
    assert!(stdout.contains("gambit sync"));
    assert!(stdout.contains("--lichess-user <NAME>"));
    assert!(stdout.contains("--max-games <N>"));
    assert!(stdout.contains("--output <DIRECTORY>"));
    assert!(stdout.contains("--format <human|json|jsonl|github>"));
    assert!(stdout.contains("--keep-going"));
    assert!(stdout.contains("--max-errors <N>"));
    assert!(stdout.contains("--position <FEN>"));
    assert!(stdout.contains("--update"));
    assert!(stdout.contains("Directories are scanned recursively"));
}

#[test]
fn index_round_trips_queries_and_pgn() {
    let directory = TestDirectory::new();
    let pgn = b"[Event \"Keep\"]\n[Date \"2026.01.02\"]\n[White \"Opponent\"]\n[Black \"Diego\"]\n[WhiteElo \"1300\"]\n[BlackElo \"1190\"]\n\n1. e4 e5 2. Nf3 Nf6 3. Ng1 Ng8 1-0\n\n[Event \"Skip\"]\n[Date \"2025.01.02\"]\n[White \"Diego\"]\n[Black \"Other\"]\n\n1. d4 d5 1-0\n";
    let compressed = zstd::stream::encode_all(&pgn[..], 1).expect("compress PGN");
    let input = directory.write("games.pgn.zst", &compressed);
    let database = directory.path().join("games.gambit");
    let built = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["index", "--format=json", "--output"])
        .arg(&database)
        .arg(&input)
        .output()
        .expect("build index");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&built.stdout).unwrap();
    assert_eq!(report["status"], "complete");
    assert_eq!(report["games"], 2);
    assert_eq!(report["sources"], 1);
    assert_eq!(report["positions"], 10);
    assert_eq!(report["pgn_bytes"], pgn.len());
    assert_eq!(report["scanned_pgn_bytes"], pgn.len());
    assert!(report["database_bytes"].as_u64().unwrap() > 0);

    let count = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args([
            "query",
            "--player=diego",
            "--color=black",
            "--result=loss",
            "--since=2026-01-01",
            "--format=count",
        ])
        .arg(&database)
        .output()
        .expect("query index");
    assert!(count.status.success());
    assert_eq!(count.stdout, b"1\n");

    let position = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args([
            "query",
            "--position",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 50 99",
            "--player=diego",
            "--color=black",
            "--format=jsonl",
        ])
        .arg(&database)
        .output()
        .expect("query indexed position");
    assert!(position.status.success());
    let record: serde_json::Value = serde_json::from_slice(&position.stdout).unwrap();
    assert_eq!(record["event"], "Keep");
    assert_eq!(record["position_ply"], 0);

    let pgn = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .arg("query")
        .arg(&database)
        .output()
        .expect("extract indexed PGN");
    assert!(pgn.status.success());
    let validation = run_with_stdin(&["doctor", "-"], &pgn.stdout);
    assert!(validation.status.success());
    assert!(String::from_utf8_lossy(&pgn.stdout).contains("[Event \"Keep\"]"));
    assert!(String::from_utf8_lossy(&pgn.stdout).contains("[Event \"Skip\"]"));
}

#[test]
fn index_refuses_overwrite_and_cleans_up_failed_builds() {
    let directory = TestDirectory::new();
    let input = directory.write("valid.pgn", b"1. e4 *\n");
    let database = directory.write("existing.gambit", b"keep me");
    let overwrite = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["index", "--output"])
        .arg(&database)
        .arg(&input)
        .output()
        .expect("attempt overwrite");
    assert_eq!(overwrite.status.code(), Some(3));
    assert_eq!(fs::read(&database).unwrap(), b"keep me");

    let invalid = directory.write("invalid.pgn", b"1. e5 *\n");
    let failed_database = directory.path().join("failed.gambit");
    let failed = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["index", "--output"])
        .arg(&failed_database)
        .arg(&invalid)
        .output()
        .expect("build invalid index");
    assert_eq!(failed.status.code(), Some(1));
    assert!(!failed_database.exists());
    assert_eq!(
        fs::read_dir(directory.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
            .count(),
        0
    );
}

#[test]
fn index_updates_only_new_and_changed_sources_and_rolls_back_failures() {
    let directory = TestDirectory::new();
    let input = directory.write(
        "a.pgn",
        b"[Event \"Original\"]\n[White \"A\"]\n[Black \"B\"]\n\n1. e4 e5 *\n",
    );
    let database = directory.path().join("games.gambit");
    let built = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["index", "--output"])
        .arg(&database)
        .arg(&input)
        .output()
        .expect("build database");
    assert!(built.status.success());

    let unchanged = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["index", "--update", "--format=json", "--output"])
        .arg(&database)
        .arg(&input)
        .output()
        .expect("update unchanged database");
    assert!(unchanged.status.success());
    let report: serde_json::Value = serde_json::from_slice(&unchanged.stdout).unwrap();
    assert_eq!(report["mode"], "update");
    assert_eq!(report["sources"], 0);
    assert_eq!(report["skipped_sources"], 1);
    assert_eq!(report["replaced_sources"], 0);
    assert_eq!(report["games"], 0);
    assert_eq!(report["pgn_bytes"], 0);
    assert_eq!(
        report["scanned_pgn_bytes"],
        fs::metadata(&input).unwrap().len()
    );

    let added = directory.write(
        "b.pgn",
        b"[Event \"Added\"]\n[White \"C\"]\n[Black \"D\"]\n\n1. d4 d5 *\n",
    );
    let appended = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["index", "--update", "--format=json", "--output"])
        .arg(&database)
        .arg(&added)
        .output()
        .expect("append source");
    assert!(appended.status.success());
    let report: serde_json::Value = serde_json::from_slice(&appended.stdout).unwrap();
    assert_eq!(report["sources"], 1);
    assert_eq!(report["games"], 1);
    assert_eq!(report["skipped_sources"], 0);

    fs::write(
        &input,
        b"[Event \"Changed\"]\n[White \"A\"]\n[Black \"B\"]\n\n1. c4 c5 *\n",
    )
    .unwrap();
    let replaced = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["index", "--update", "--format=json", "--output"])
        .arg(&database)
        .arg(&input)
        .output()
        .expect("replace source");
    assert!(replaced.status.success());
    let report: serde_json::Value = serde_json::from_slice(&replaced.stdout).unwrap();
    assert_eq!(report["sources"], 1);
    assert_eq!(report["replaced_sources"], 1);
    assert_eq!(report["games"], 1);

    let contents = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["query", "--format=jsonl"])
        .arg(&database)
        .output()
        .expect("query updated database");
    assert!(contents.status.success());
    let contents = String::from_utf8(contents.stdout).unwrap();
    assert!(!contents.contains("Original"));
    assert!(contents.contains("Changed"));
    assert!(contents.contains("Added"));
    assert_eq!(contents.lines().count(), 2);

    fs::write(&input, b"1. e5 *\n").unwrap();
    let failed = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["index", "--update", "--output"])
        .arg(&database)
        .arg(&input)
        .output()
        .expect("reject invalid changed source");
    assert_eq!(failed.status.code(), Some(1));

    let after_failure = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["query", "--format=jsonl"])
        .arg(&database)
        .output()
        .expect("query after rolled-back update");
    assert!(after_failure.status.success());
    let after_failure = String::from_utf8(after_failure.stdout).unwrap();
    assert!(after_failure.contains("Changed"));
    assert!(after_failure.contains("Added"));
    assert_eq!(after_failure.lines().count(), 2);
}

#[test]
fn query_rejects_a_non_gambit_database() {
    let file = TestFile::new("gambit", b"not a database");
    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["query", "--format=count"])
        .arg(file.path())
        .output()
        .expect("query invalid database");

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Gambit database"));
}

#[test]
fn stats_summarizes_mainlines_and_results_as_json() {
    let output = run_with_stdin(
        &["stats", "--format=json", "-"],
        b"1. e4 (1. d4 d5 1/2-1/2) e5 1-0\n\n1. d4 0-1\n\n*\n",
    );

    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["status"], "valid");
    assert_eq!(report["source"], "stdin");
    assert_eq!(report["outcome_required"], true);
    assert_eq!(report["games"], 3);
    assert_eq!(report["mainline_plies"], 3);
    assert_eq!(report["results"]["white_wins"], 1);
    assert_eq!(report["results"]["black_wins"], 1);
    assert_eq!(report["results"]["draws"], 0);
    assert_eq!(report["results"]["unfinished"], 1);
    assert_eq!(report["game_length"]["minimum_plies"], 0);
    assert_eq!(report["game_length"]["average_plies"], 1.0);
    assert_eq!(report["game_length"]["maximum_plies"], 2);
}

#[test]
fn stats_human_output_is_a_compact_summary() {
    let output = run_with_stdin(&["stats", "-"], b"1. e4 e5 1/2-1/2\n");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("stats: valid"));
    assert!(stdout.contains("games: 1"));
    assert!(stdout.contains("mainline plies: 2"));
    assert!(stdout.contains("0 white wins, 0 black wins, 1 draw, 0 unfinished"));
    assert!(stdout.contains("min 2, avg 2.00, max 2"));
    assert!(stdout.contains("game-length buckets: 0=0, 1-20=1"));
    assert!(stdout.contains("header coverage:"));
    assert!(stdout.contains("dates: 0 complete, 0 incomplete/invalid, 1 missing"));
    assert!(stdout.contains("ratings: 0 numeric, 0 invalid, 2 missing"));
    assert!(stdout.contains("rating bands: <1000=0"));
    assert!(stdout.contains("time controls:") && stdout.contains("missing=1"));
}

#[test]
fn stats_returns_partial_counts_for_invalid_pgn() {
    let output = run_with_stdin(&["stats", "--format=json", "-"], b"1. e4 *\n\n1. d4\n");

    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "invalid");
    assert_eq!(report["games"], 1);
    assert_eq!(report["mainline_plies"], 1);
    assert_eq!(report["diagnostic"]["category"], "syntax");
    assert_eq!(report["diagnostic"]["byte"], 15);
}

#[test]
fn stats_lenient_mode_counts_a_final_game_without_an_outcome() {
    let output = run_with_stdin(&["stats", "--lenient", "--format=json", "-"], b"1. e4 e5\n");

    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["games"], 1);
    assert_eq!(report["mainline_plies"], 2);
    assert_eq!(report["results"]["unfinished"], 1);
    assert_eq!(report["outcome_required"], false);
}

#[test]
fn stats_is_lexical_and_does_not_reject_illegal_chess_moves() {
    let output = run_with_stdin(&["stats", "--format=json", "-"], b"1. e5 Ke7 2. Ke2 1-0\n");

    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "valid");
    assert_eq!(report["games"], 1);
    assert_eq!(report["mainline_plies"], 3);
}

#[test]
fn stats_reads_zstd_files() {
    let pgn = b"1. e4 e5 1-0\n";
    let compressed = zstd::stream::encode_all(&pgn[..], 1).expect("compress PGN");
    let file = TestFile::new("pgn.zst", &compressed);
    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["stats", "--format=json"])
        .arg(file.path())
        .output()
        .expect("run gambit");

    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["bytes"], pgn.len());
    assert_eq!(report["games"], 1);
    assert_eq!(report["results"]["white_wins"], 1);
}

#[test]
fn stats_aggregates_a_directory_batch() {
    let directory = TestDirectory::new();
    directory.write(
        "one.pgn",
        b"[Event \"One\"]\n[Date \"2025.01.02\"]\n[WhiteElo \"2000\"]\n[BlackElo \"2200\"]\n[TimeControl \"180+0\"]\n\n1. e4 *\n",
    );
    directory.write(
        "nested/two.pgn",
        b"[Date \"2023.12.31\"]\n[WhiteElo \"?\"]\n[BlackElo \"1800\"]\n[TimeControl \"40/7200:3600\"]\n\n1. d4 d5 0-1\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["stats", "--format=json"])
        .arg(directory.path())
        .output()
        .expect("run gambit");

    assert!(output.status.success());
    let batch: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(batch["status"], "valid");
    assert_eq!(batch["input_count"], 2);
    assert_eq!(batch["valid_input_count"], 2);
    assert_eq!(batch["games"], 2);
    assert_eq!(batch["mainline_plies"], 3);
    assert_eq!(batch["results"]["black_wins"], 1);
    assert_eq!(batch["game_length"]["average_plies"], 1.5);
    assert_eq!(batch["header_coverage"]["event"], 1);
    assert_eq!(batch["header_coverage"]["date"], 2);
    assert_eq!(batch["dates"]["complete"], 2);
    assert_eq!(batch["dates"]["earliest"], "2023.12.31");
    assert_eq!(batch["dates"]["latest"], "2025.01.02");
    assert_eq!(batch["ratings"]["numeric"], 3);
    assert_eq!(batch["ratings"]["invalid"], 1);
    assert_eq!(batch["ratings"]["missing"], 0);
    assert_eq!(batch["ratings"]["minimum"], 1800);
    assert_eq!(batch["ratings"]["average"], 2000.0);
    assert_eq!(batch["ratings"]["maximum"], 2200);
    assert_eq!(batch["ratings"]["distribution"]["from_1800_to_1999"], 1);
    assert_eq!(batch["ratings"]["distribution"]["from_2000_to_2199"], 1);
    assert_eq!(batch["ratings"]["distribution"]["from_2200_to_2399"], 1);
    assert_eq!(batch["game_length"]["distribution"]["from_1_to_20"], 2);
    assert_eq!(batch["time_controls"]["increment"], 1);
    assert_eq!(batch["time_controls"]["multi_stage"], 1);
    assert_eq!(batch["reports"].as_array().unwrap().len(), 2);
}

#[test]
fn stats_batch_preserves_counts_and_most_severe_status() {
    let valid = TestFile::new("pgn", b"1. e4 *\n");
    let invalid = TestFile::new("pgn", b"1. d4\n");
    let missing = std::env::temp_dir().join(format!(
        "gambit-cli-missing-{}-{}.pgn",
        std::process::id(),
        NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["stats", "--format=json"])
        .arg(valid.path())
        .arg(invalid.path())
        .arg(&missing)
        .output()
        .expect("run gambit");

    assert_eq!(output.status.code(), Some(3));
    let batch: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(batch["status"], "error");
    assert_eq!(batch["valid_input_count"], 1);
    assert_eq!(batch["invalid_input_count"], 1);
    assert_eq!(batch["error_input_count"], 1);
    assert_eq!(batch["games"], 1);
    assert_eq!(batch["mainline_plies"], 1);
}

#[test]
fn stats_rejects_streaming_jsonl_until_it_has_a_record_contract() {
    let output = run_with_stdin(&["stats", "--format=jsonl", "-"], b"1. e4 *\n");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown stats output format"));
}

#[test]
fn query_counts_player_relative_matches() {
    let input = b"[Date \"2026.01.02\"]\n[White \"Opponent\"]\n[Black \"DiegoGLozano\"]\n[WhiteElo \"1300\"]\n[BlackElo \"1190\"]\n\n1. e4 1-0\n\n[Date \"2025.01.02\"]\n[White \"diegoglozano\"]\n[Black \"Other\"]\n[WhiteElo \"1200\"]\n[BlackElo \"1250\"]\n\n1. d4 1-0\n";
    let output = run_with_stdin(
        &[
            "query",
            "-",
            "--player=diegoglozano",
            "--color",
            "black",
            "--result=loss",
            "--since",
            "2026-01-01",
            "--min-rating=1100",
            "--max-rating",
            "1200",
            "--format=count",
        ],
        input,
    );

    assert!(output.status.success());
    assert_eq!(output.stdout, b"1\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn query_emits_matching_pgn_that_can_be_validated() {
    let input = b"[Event \"Keep\"]\n[White \"Diego\"]\n[Black \"Target\"]\n\n1. e4 *\n\n[Event \"Skip\"]\n[White \"Diego\"]\n[Black \"Other\"]\n\n1. d4 *\n";
    let output = run_with_stdin(
        &["query", "--player", "diego", "--opponent", "target", "-"],
        input,
    );

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("[Event \"Keep\"]"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("[Event \"Skip\"]"));
    let validation = run_with_stdin(&["doctor", "--syntax-only", "-"], &output.stdout);
    assert!(validation.status.success());
}

#[test]
fn query_jsonl_identifies_each_matching_game() {
    let input = b"[Site \"https://example.test/a\"]\n[UTCDate \"2026.02.03\"]\n[White \"A\"]\n[Black \"B\"]\n[WhiteElo \"1500\"]\n[BlackElo \"?\"]\n\n1. e4 e5 1/2-1/2\n";
    let output = run_with_stdin(
        &["query", "--result", "draw", "--format", "jsonl", "-"],
        input,
    );

    assert!(output.status.success());
    let record: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(record["schema_version"], 1);
    assert_eq!(record["source"], "stdin");
    assert_eq!(record["game"], 1);
    assert_eq!(record["date"], "2026.02.03");
    assert_eq!(record["white_elo"], 1500);
    assert!(record.get("black_elo").is_none());
    assert_eq!(record["result"], "draw");
    assert_eq!(record["mainline_plies"], 2);
}

#[test]
fn query_rejects_player_relative_filters_without_a_player() {
    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["query", "--result", "win", "example.pgn"])
        .output()
        .expect("run gambit");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("require --player"));
}

#[test]
fn query_counts_plain_and_zstd_games_in_a_directory() {
    let directory = TestDirectory::new();
    directory.write("one.pgn", b"[White \"Diego\"]\n[Black \"A\"]\n\n1. e4 *\n");
    let second = b"[White \"B\"]\n[Black \"Diego\"]\n\n1. d4 *\n";
    let compressed = zstd::stream::encode_all(&second[..], 1).expect("compress PGN");
    directory.write("nested/two.pgn.zst", &compressed);

    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["query", "--player", "diego", "--format", "count"])
        .arg(directory.path())
        .output()
        .expect("run gambit");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"2\n");
}

#[test]
fn query_counts_games_reaching_a_position_and_composes_filters() {
    let input = b"[White \"Diego\"]\n[Black \"A\"]\n\n1. e4 e5 2. Nf3 *\n\n[White \"Other\"]\n[Black \"B\"]\n\n1. e4 e5 *\n";
    let output = run_with_stdin(
        &[
            "query",
            "--position",
            "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 99 42",
            "--player",
            "diego",
            "--format",
            "count",
            "-",
        ],
        input,
    );

    assert!(output.status.success());
    assert_eq!(output.stdout, b"1\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn query_rejects_an_invalid_position_argument() {
    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["query", "--position", "not-a-fen", "example.pgn"])
        .output()
        .expect("run gambit");

    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("--position must be a valid six-field FEN")
    );
}

#[test]
fn position_query_reports_an_illegal_mainline_move() {
    let output = run_with_stdin(
        &[
            "query",
            "--position",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "--format",
            "count",
            "-",
        ],
        b"1. e5 *\n",
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("game 1, ply 1"));
    assert!(stderr.contains("SAN does not identify a legal move (e5)"));
}

#[test]
fn count_query_does_not_emit_a_partial_total_on_failure() {
    let output = run_with_stdin(
        &["query", "--result", "unfinished", "--format", "count", "-"],
        b"[Event \"Complete match\"]\n\n1. e4 *\n\n[Event \"Broken game\"]\n\n1. e4 (\n",
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("PGN stream ended"));
}

#[test]
fn reports_the_release_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .arg("--version")
        .output()
        .expect("run gambit");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"gambit 0.7.0\n");
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
fn github_format_emits_an_annotation_and_summary() {
    let file = TestFile::new("bad,name.pgn", b"1. e5 *\n");
    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["doctor", "--format=github"])
        .arg(file.path())
        .output()
        .expect("run gambit");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("::error file="));
    assert!(lines[0].contains("%2C"));
    assert!(lines[0].contains(
        ",line=1,col=4,title=Gambit Doctor%3A illegal move::game 1, ply 1: SAN does not identify a legal move (e5)"
    ));
    assert!(lines[1].starts_with("Gambit Doctor: invalid - "));
    assert!(lines[1].ends_with("; 1 diagnostic, 0 complete games, 0 moves"));
}

#[test]
fn github_format_summarizes_valid_input_without_an_annotation() {
    let output = run_with_stdin(&["doctor", "--format=github", "-"], b"1. e4 *\n");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        b"Gambit Doctor: valid - stdin; 0 diagnostics, 1 complete game, 1 move\n"
    );
}

#[test]
fn diagnoses_illegal_moves_as_json() {
    let output = run_with_stdin(
        &["doctor", "--format=json", "-"],
        b"[Event \"Example\"]\n[Date \"2026.09.02\"]\n[White \"Alice\"]\n[Black \"Bob\"]\n\n1. e5 *\n",
    );
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "invalid");
    assert_eq!(report["diagnostic"]["category"], "illegal_move");
    assert_eq!(report["diagnostic"]["game"], 1);
    assert_eq!(report["diagnostic"]["ply"], 1);
    assert_eq!(report["diagnostic"]["context"], "e5");
    assert_eq!(report["diagnostic"]["line"], 6);
    assert_eq!(report["diagnostic"]["column"], 4);
    assert_eq!(report["diagnostic"]["excerpt"], "1. e5 *");
    assert_eq!(report["diagnostic"]["headers"]["event"], "Example");
    assert_eq!(report["diagnostic"]["headers"]["date"], "2026.09.02");
    assert_eq!(report["diagnostic"]["headers"]["white"], "Alice");
    assert_eq!(report["diagnostic"]["headers"]["black"], "Bob");
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
    assert_eq!(report["diagnostic"]["line"], 3);
    assert_eq!(report["diagnostic"]["column"], 7);
    assert_eq!(report["diagnostic"]["excerpt"], "1. e4 ) *");
}

#[test]
fn human_diagnostics_include_game_and_source_context() {
    let output = run_with_stdin(
        &["doctor", "-"],
        b"[Event \"Club Championship\"]\n[White \"Alice\"]\n[Black \"Bob\"]\n\n1. e5 *\n",
    );
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("line 5, column 4"));
    assert!(stderr.contains("White=\"Alice\", Black=\"Bob\""));
    assert!(stderr.contains("Event=\"Club Championship\""));
    assert!(stderr.contains("source: 1. e5 *"));
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

#[test]
fn keep_going_reports_errors_from_multiple_games() {
    let output = run_with_stdin(
        &["doctor", "--keep-going", "--format=json", "-"],
        b"[Event \"First\"]\n\n1. e5 *\n\n[Event \"Second\"]\n\n1. d5 *\n\n1. e4 e5 *\n",
    );
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["games"], 3);
    assert_eq!(report["diagnostic_count"], 2);
    assert_eq!(report["diagnostic"]["game"], 1);
    assert_eq!(report["diagnostic"]["headers"]["event"], "First");
    assert_eq!(report["additional_diagnostics"][0]["game"], 2);
    assert_eq!(
        report["additional_diagnostics"][0]["headers"]["event"],
        "Second"
    );
    assert_eq!(report["additional_diagnostics"][0]["line"], 7);
    assert_eq!(report["error_limit_reached"], false);
}

#[test]
fn max_errors_limits_a_complete_scan() {
    let output = run_with_stdin(
        &["doctor", "--max-errors=2", "--format=json", "-"],
        b"1. e5 *\n1. d5 *\n1. c5 *\n",
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["games"], 2);
    assert_eq!(report["diagnostic_count"], 2);
    assert_eq!(report["error_limit_reached"], true);
}

#[test]
fn jsonl_emits_diagnostics_followed_by_a_summary() {
    let output = run_with_stdin(
        &["doctor", "--keep-going", "--format=jsonl", "-"],
        b"1. e5 *\n1. d5 *\n",
    );
    assert_eq!(output.status.code(), Some(1));
    let records = output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0]["record"], "diagnostic");
    assert_eq!(records[0]["diagnostic"]["game"], 1);
    assert_eq!(records[1]["record"], "diagnostic");
    assert_eq!(records[1]["diagnostic"]["game"], 2);
    assert_eq!(records[2]["record"], "summary");
    assert_eq!(records[2]["diagnostic_count"], 2);
}

#[test]
fn complete_lenient_scan_accepts_a_final_game_without_an_outcome() {
    let output = run_with_stdin(
        &["doctor", "--keep-going", "--lenient", "--format=json", "-"],
        b"1. e4 *\n1. d4 d5\n",
    );
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["games"], 2);
    assert_eq!(report["moves"], 3);
    assert_eq!(report["diagnostic_count"], 0);
}

#[test]
fn rejects_zero_as_an_error_limit() {
    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["doctor", "--max-errors", "0", "-"])
        .output()
        .expect("run gambit");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("positive integer"));
}

#[test]
fn reads_zstd_files_without_an_external_decompressor() {
    let pgn = b"[Event \"Compressed\"]\n\n1. e4 e5 *\n";
    let compressed = zstd::stream::encode_all(&pgn[..], 1).expect("compress PGN");
    let file = TestFile::new("pgn.zst", &compressed);
    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["doctor", "--format=json"])
        .arg(file.path())
        .output()
        .expect("run gambit");

    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "valid");
    assert_eq!(report["bytes"], pgn.len());
    assert_eq!(report["games"], 1);
    assert_eq!(report["moves"], 2);
}

#[test]
fn reports_corrupt_zstd_as_an_input_error() {
    let file = TestFile::new("pgn.zst", b"not a zstd frame");
    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["doctor", "--format=json"])
        .arg(file.path())
        .output()
        .expect("run gambit");

    assert_eq!(output.status.code(), Some(3));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "error");
    assert_eq!(report["diagnostic"]["category"], "input");
}

#[test]
fn multi_file_json_aggregates_reports_and_exit_status() {
    let valid = TestFile::new("pgn", b"1. e4 *\n");
    let invalid = TestFile::new("pgn", b"1. e5 *\n");
    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["doctor", "--format=json"])
        .arg(valid.path())
        .arg(invalid.path())
        .output()
        .expect("run gambit");

    assert_eq!(output.status.code(), Some(1));
    let batch: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(batch["schema_version"], 1);
    assert_eq!(batch["status"], "invalid");
    assert_eq!(batch["input_count"], 2);
    assert_eq!(batch["games"], 1);
    assert_eq!(batch["diagnostic_count"], 1);
    assert_eq!(batch["reports"][0]["status"], "valid");
    assert_eq!(batch["reports"][1]["status"], "invalid");
    assert_eq!(
        batch["reports"][1]["diagnostic"]["category"],
        "illegal_move"
    );
}

#[test]
fn multi_file_scan_continues_after_an_input_error() {
    let valid = TestFile::new("pgn", b"1. e4 *\n");
    let missing = std::env::temp_dir().join(format!(
        "gambit-cli-missing-{}-{}.pgn",
        std::process::id(),
        NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["doctor", "--format=json"])
        .arg(&missing)
        .arg(valid.path())
        .output()
        .expect("run gambit");

    assert_eq!(output.status.code(), Some(3));
    let batch: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(batch["status"], "error");
    assert_eq!(batch["reports"][0]["status"], "error");
    assert_eq!(batch["reports"][1]["status"], "valid");
}

#[test]
fn multi_file_jsonl_ends_with_a_batch_summary() {
    let first = TestFile::new("pgn", b"1. e4 *\n");
    let second = TestFile::new("pgn", b"1. d4 *\n");
    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["doctor", "--format=jsonl"])
        .arg(first.path())
        .arg(second.path())
        .output()
        .expect("run gambit");

    assert!(output.status.success());
    let records = output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0]["record"], "summary");
    assert_eq!(records[1]["record"], "summary");
    assert_eq!(records[2]["record"], "batch_summary");
    assert_eq!(records[2]["input_count"], 2);
    assert_eq!(records[2]["games"], 2);
}

#[test]
fn recursively_scans_pgn_files_in_deterministic_order() {
    let directory = TestDirectory::new();
    directory.write("z-last.pgn", b"1. e4 *\n");
    let compressed = zstd::stream::encode_all(&b"1. d4 *\n"[..], 1).expect("compress PGN");
    directory.write("nested/a-first.PGN.ZST", &compressed);
    directory.write("nested/ignored.zst", b"not a PGN");
    directory.write("notes.txt", b"not a PGN");

    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["doctor", "--format=json"])
        .arg(directory.path())
        .output()
        .expect("run gambit");

    assert!(output.status.success());
    let batch: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(batch["status"], "valid");
    assert_eq!(batch["input_count"], 2);
    assert_eq!(batch["games"], 2);
    assert_eq!(batch["moves"], 2);
    assert_eq!(
        Path::new(batch["reports"][0]["source"].as_str().unwrap())
            .file_name()
            .unwrap(),
        "a-first.PGN.ZST"
    );
    assert_eq!(
        Path::new(batch["reports"][1]["source"].as_str().unwrap())
            .file_name()
            .unwrap(),
        "z-last.pgn"
    );
}

#[test]
fn reports_an_empty_directory_as_an_input_error() {
    let directory = TestDirectory::new();
    directory.write("notes.txt", b"not a PGN");

    let output = Command::new(env!("CARGO_BIN_EXE_gambit"))
        .args(["doctor", "--format=json"])
        .arg(directory.path())
        .output()
        .expect("run gambit");

    assert_eq!(output.status.code(), Some(3));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "error");
    assert_eq!(report["diagnostic"]["category"], "input");
    assert!(
        report["diagnostic"]["message"]
            .as_str()
            .unwrap()
            .contains("no .pgn or .pgn.zst files found")
    );
}

#[test]
fn reports_cross_field_consistency_errors() {
    let output = run_with_stdin(
        &["doctor", "--keep-going", "--format=json", "-"],
        b"[Result \"1-0\"]\n\n*\n\n[SetUp \"1\"]\n\n*\n\n1. e4 e5 4. Nf3 *\n",
    );
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostic_count"], 3);
    assert_eq!(report["diagnostic"]["category"], "inconsistent_result");
    assert_eq!(
        report["additional_diagnostics"][0]["category"],
        "inconsistent_setup"
    );
    assert_eq!(
        report["additional_diagnostics"][1]["category"],
        "incorrect_move_number"
    );
    assert_eq!(report["additional_diagnostics"][1]["context"], "4.");
}
