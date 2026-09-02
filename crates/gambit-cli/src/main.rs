mod doctor;

use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use doctor::{
    Diagnostic, DoctorOptions, GameHeaders, Report, ReportStatus, ValidationMode, inspect,
};
use serde::Serialize;

const DEFAULT_KEEP_GOING_ERRORS: usize = 100;
const USAGE: &str = "Usage:\n  gambit doctor [OPTIONS] <FILE.pgn|FILE.pgn.zst|->...\n  gambit <FILE.pgn|FILE.pgn.zst|->\n\nCommands:\n  doctor    Diagnose PGN syntax and chess-semantic errors\n\nThe direct file form is retained as a compatibility alias for 'gambit doctor'.\nFiles ending in .zst are decompressed automatically. Use - alone to read PGN from standard input.\n\nOptions:\n      --format <human|json|jsonl>  Select output format [default: human]\n      --syntax-only                Check PGN structure without executing moves\n      --lenient                    Allow a final game without an outcome marker\n      --keep-going                 Continue after errors [default limit: 100]\n      --max-errors <N>             Continue until N errors have been reported per input\n  -q, --quiet                      Print nothing when the input is valid\n  -h, --help                       Print help\n  -V, --version                    Print version";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutputFormat {
    Human,
    Json,
    Jsonl,
}

#[derive(Debug)]
struct DoctorCommand {
    paths: Vec<OsString>,
    format: OutputFormat,
    quiet: bool,
    options: DoctorOptions,
}

#[derive(Debug)]
enum Action {
    Help,
    Version,
    Doctor(DoctorCommand),
}

fn main() -> ExitCode {
    match parse_arguments(env::args_os().skip(1)) {
        Ok(Action::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Action::Version) => {
            println!("gambit {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Action::Doctor(command)) => run_doctor(&command),
        Err(error) => {
            eprintln!("{error}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn parse_arguments(arguments: impl Iterator<Item = OsString>) -> Result<Action, String> {
    let mut arguments = arguments.peekable();
    let Some(first) = arguments.next() else {
        return Err(String::from("missing command or PGN file"));
    };
    match first.to_str() {
        Some("-h" | "--help") => no_more(arguments, Action::Help),
        Some("-V" | "--version") => no_more(arguments, Action::Version),
        Some("doctor") => parse_doctor_arguments(arguments),
        Some(value) if value.starts_with('-') && value != "-" => {
            Err(format!("unknown option or command: {value}"))
        }
        _ => {
            if arguments.next().is_some() {
                return Err(String::from(
                    "the compatibility form accepts exactly one PGN file; use 'gambit doctor' for options",
                ));
            }
            Ok(Action::Doctor(DoctorCommand {
                paths: vec![first],
                format: OutputFormat::Human,
                quiet: false,
                options: DoctorOptions {
                    mode: ValidationMode::Semantic,
                    require_outcome: true,
                    max_errors: 1,
                },
            }))
        }
    }
}

fn no_more(
    mut arguments: impl Iterator<Item = OsString>,
    action: Action,
) -> Result<Action, String> {
    if arguments.next().is_some() {
        Err(String::from("unexpected argument"))
    } else {
        Ok(action)
    }
}

fn parse_doctor_arguments(arguments: impl Iterator<Item = OsString>) -> Result<Action, String> {
    let mut paths = Vec::new();
    let mut format = OutputFormat::Human;
    let mut quiet = false;
    let mut mode = ValidationMode::Semantic;
    let mut require_outcome = true;
    let mut max_errors = 1;
    let mut arguments = arguments.peekable();

    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("-h" | "--help") => return no_more(arguments, Action::Help),
            Some("-q" | "--quiet") => quiet = true,
            Some("--syntax-only") => mode = ValidationMode::Syntax,
            Some("--lenient") => require_outcome = false,
            Some("--keep-going") => max_errors = DEFAULT_KEEP_GOING_ERRORS,
            Some("--max-errors") => {
                let value = arguments
                    .next()
                    .ok_or_else(|| String::from("--max-errors requires a value"))?;
                max_errors = parse_max_errors(&value)?;
            }
            Some("--format") => {
                let value = arguments
                    .next()
                    .ok_or_else(|| String::from("--format requires a value"))?;
                format = parse_format(&value)?;
            }
            Some("--") => {
                paths.extend(arguments);
                break;
            }
            Some(value) if value.starts_with("--format=") => {
                format = parse_format(std::ffi::OsStr::new(&value["--format=".len()..]))?;
            }
            Some(value) if value.starts_with("--max-errors=") => {
                max_errors =
                    parse_max_errors(std::ffi::OsStr::new(&value["--max-errors=".len()..]))?;
            }
            Some(value) if value.starts_with('-') && value != "-" => {
                return Err(format!("unknown doctor option: {value}"));
            }
            _ => paths.push(argument),
        }
    }

    if quiet && format != OutputFormat::Human {
        return Err(String::from(
            "--quiet cannot be combined with a machine-readable format",
        ));
    }
    if paths.is_empty() {
        return Err(String::from("doctor requires at least one PGN file"));
    }
    if paths.len() > 1 && paths.iter().any(|path| path == "-") {
        return Err(String::from(
            "standard input cannot be combined with other PGN files",
        ));
    }
    Ok(Action::Doctor(DoctorCommand {
        paths,
        format,
        quiet,
        options: DoctorOptions {
            mode,
            require_outcome,
            max_errors,
        },
    }))
}

fn parse_format(value: &std::ffi::OsStr) -> Result<OutputFormat, String> {
    match value.to_str() {
        Some("human") => Ok(OutputFormat::Human),
        Some("json") => Ok(OutputFormat::Json),
        Some("jsonl") => Ok(OutputFormat::Jsonl),
        Some(value) => Err(format!("unknown output format: {value}")),
        None => Err(String::from("output format must be valid UTF-8")),
    }
}

fn parse_max_errors(value: &std::ffi::OsStr) -> Result<usize, String> {
    let value = value
        .to_str()
        .ok_or_else(|| String::from("error limit must be valid UTF-8"))?;
    match value.parse::<usize>() {
        Ok(0) | Err(_) => Err(String::from("--max-errors must be a positive integer")),
        Ok(limit) => Ok(limit),
    }
}

fn run_doctor(command: &DoctorCommand) -> ExitCode {
    let reports = if command.paths[0] == "-" {
        vec![inspect(
            io::stdin().lock(),
            String::from("stdin"),
            command.options,
        )]
    } else {
        command
            .paths
            .iter()
            .map(|path| inspect_path(path, command.options))
            .collect::<Vec<_>>()
    };
    let exit_code = reports.iter().map(Report::exit_code).max().unwrap_or(0);
    if let Err(error) = render_reports(&reports, command.format, command.quiet) {
        eprintln!("failed to write report: {error}");
        return ExitCode::from(3);
    }
    ExitCode::from(exit_code)
}

fn inspect_path(path: &std::ffi::OsStr, options: DoctorOptions) -> Report {
    let source = path.to_string_lossy().into_owned();
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) => {
            return Report::input_error(
                source,
                options,
                format!("failed to open {}: {error}", path.to_string_lossy()),
            );
        }
    };
    if is_zstd_path(path) {
        match zstd::stream::read::Decoder::new(file) {
            Ok(decoder) => inspect(decoder, source, options),
            Err(error) => Report::input_error(
                source,
                options,
                format!(
                    "failed to initialize zstd decoder for {}: {error}",
                    path.to_string_lossy()
                ),
            ),
        }
    } else {
        inspect(file, source, options)
    }
}

fn is_zstd_path(path: &std::ffi::OsStr) -> bool {
    Path::new(path)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zst"))
}

fn aggregate_status(reports: &[Report]) -> ReportStatus {
    if reports
        .iter()
        .any(|report| report.status == ReportStatus::Error)
    {
        ReportStatus::Error
    } else if reports
        .iter()
        .any(|report| report.status == ReportStatus::Invalid)
    {
        ReportStatus::Invalid
    } else {
        ReportStatus::Valid
    }
}

#[derive(Serialize)]
struct BatchReport<'a> {
    schema_version: u8,
    status: ReportStatus,
    input_count: usize,
    bytes: u64,
    games: u64,
    moves: u64,
    elapsed_seconds: f64,
    throughput_mib_per_second: f64,
    diagnostic_count: usize,
    reports: &'a [Report],
}

impl<'a> BatchReport<'a> {
    fn new(reports: &'a [Report]) -> Self {
        let bytes = reports.iter().map(|report| report.bytes).sum();
        let elapsed_seconds = reports
            .iter()
            .map(|report| report.elapsed_seconds)
            .sum::<f64>();
        #[allow(clippy::cast_precision_loss)]
        let throughput_mib_per_second = if elapsed_seconds > 0.0 {
            bytes as f64 / (1024.0 * 1024.0) / elapsed_seconds
        } else {
            0.0
        };
        Self {
            schema_version: 1,
            status: aggregate_status(reports),
            input_count: reports.len(),
            bytes,
            games: reports.iter().map(|report| report.games).sum(),
            moves: reports.iter().map(|report| report.moves).sum(),
            elapsed_seconds,
            throughput_mib_per_second,
            diagnostic_count: reports.iter().map(|report| report.diagnostic_count).sum(),
            reports,
        }
    }
}

fn render_reports(reports: &[Report], format: OutputFormat, quiet: bool) -> io::Result<()> {
    if reports.len() == 1 {
        return render_report(&reports[0], format, quiet);
    }
    match format {
        OutputFormat::Human => {
            for report in reports {
                render_report(report, format, quiet)?;
            }
            Ok(())
        }
        OutputFormat::Json => {
            let mut output = io::stdout().lock();
            serde_json::to_writer(&mut output, &BatchReport::new(reports))?;
            writeln!(output)
        }
        OutputFormat::Jsonl => render_jsonl_batch(reports),
    }
}

fn render_report(report: &Report, format: OutputFormat, quiet: bool) -> io::Result<()> {
    match format {
        OutputFormat::Json => {
            let mut output = io::stdout().lock();
            serde_json::to_writer(&mut output, report)?;
            writeln!(output)
        }
        OutputFormat::Jsonl => render_jsonl(report),
        OutputFormat::Human if report.status == ReportStatus::Valid && quiet => Ok(()),
        OutputFormat::Human if report.status == ReportStatus::Valid => {
            let mut output = io::stdout().lock();
            writeln!(output, "valid: {}", report.source)?;
            writeln!(output, "mode: {}", report.mode)?;
            writeln!(output, "bytes: {}", report.bytes)?;
            writeln!(output, "games: {}", report.games)?;
            writeln!(output, "moves: {}", report.moves)?;
            writeln!(output, "elapsed: {:.3}s", report.elapsed_seconds)?;
            writeln!(
                output,
                "throughput: {:.2} MiB/s",
                report.throughput_mib_per_second
            )
        }
        OutputFormat::Human => {
            let mut output = io::stderr().lock();
            let label = match report.status {
                ReportStatus::Invalid => "invalid",
                ReportStatus::Error => "error",
                ReportStatus::Valid => unreachable!(),
            };
            writeln!(output, "{label}: {}", report.source)?;
            for (index, diagnostic) in report.diagnostics().enumerate() {
                if index > 0 {
                    writeln!(output)?;
                }
                render_diagnostic(&mut output, diagnostic)?;
            }
            if report.diagnostic_count > 1 {
                writeln!(output, "diagnostics: {}", report.diagnostic_count)?;
            }
            if report.error_limit_reached {
                writeln!(output, "stopped after reaching the error limit")?;
            }
            writeln!(
                output,
                "processed: {} bytes, {} complete games, {} moves",
                report.bytes, report.games, report.moves
            )
        }
    }
}

#[derive(Serialize)]
struct JsonlDiagnostic<'a> {
    schema_version: u8,
    record: &'static str,
    source: &'a str,
    diagnostic: &'a Diagnostic,
}

#[derive(Serialize)]
struct JsonlSummary<'a> {
    schema_version: u8,
    record: &'static str,
    status: ReportStatus,
    source: &'a str,
    mode: &'static str,
    outcome_required: bool,
    bytes: u64,
    games: u64,
    moves: u64,
    elapsed_seconds: f64,
    throughput_mib_per_second: f64,
    diagnostic_count: usize,
    error_limit_reached: bool,
}

#[derive(Serialize)]
struct JsonlBatchSummary {
    schema_version: u8,
    record: &'static str,
    status: ReportStatus,
    input_count: usize,
    bytes: u64,
    games: u64,
    moves: u64,
    elapsed_seconds: f64,
    throughput_mib_per_second: f64,
    diagnostic_count: usize,
}

fn render_jsonl(report: &Report) -> io::Result<()> {
    let mut output = io::stdout().lock();
    write_jsonl_report(&mut output, report)
}

fn write_jsonl_report(output: &mut impl Write, report: &Report) -> io::Result<()> {
    for diagnostic in report.diagnostics() {
        serde_json::to_writer(
            &mut *output,
            &JsonlDiagnostic {
                schema_version: report.schema_version,
                record: "diagnostic",
                source: &report.source,
                diagnostic,
            },
        )?;
        writeln!(&mut *output)?;
    }
    serde_json::to_writer(
        &mut *output,
        &JsonlSummary {
            schema_version: report.schema_version,
            record: "summary",
            status: report.status,
            source: &report.source,
            mode: report.mode,
            outcome_required: report.outcome_required,
            bytes: report.bytes,
            games: report.games,
            moves: report.moves,
            elapsed_seconds: report.elapsed_seconds,
            throughput_mib_per_second: report.throughput_mib_per_second,
            diagnostic_count: report.diagnostic_count,
            error_limit_reached: report.error_limit_reached,
        },
    )?;
    writeln!(output)
}

fn render_jsonl_batch(reports: &[Report]) -> io::Result<()> {
    let mut output = io::stdout().lock();
    for report in reports {
        write_jsonl_report(&mut output, report)?;
    }
    let batch = BatchReport::new(reports);
    serde_json::to_writer(
        &mut output,
        &JsonlBatchSummary {
            schema_version: batch.schema_version,
            record: "batch_summary",
            status: batch.status,
            input_count: batch.input_count,
            bytes: batch.bytes,
            games: batch.games,
            moves: batch.moves,
            elapsed_seconds: batch.elapsed_seconds,
            throughput_mib_per_second: batch.throughput_mib_per_second,
            diagnostic_count: batch.diagnostic_count,
        },
    )?;
    writeln!(output)
}

fn render_diagnostic(output: &mut impl Write, diagnostic: &Diagnostic) -> io::Result<()> {
    write!(output, "{}", diagnostic.category.label())?;
    if let Some(game) = diagnostic.game {
        write!(output, " at game {game}")?;
    }
    if let Some(ply) = diagnostic.ply {
        write!(output, ", ply {ply}")?;
    }
    if let Some(line) = diagnostic.line {
        write!(output, ", line {line}")?;
    }
    if let Some(column) = diagnostic.column {
        write!(output, ", column {column}")?;
    }
    if let Some(byte) = diagnostic.byte {
        write!(output, ", byte {byte}")?;
    }
    writeln!(output, ": {}", diagnostic.message)?;
    if let Some(headers) = &diagnostic.headers {
        render_game_headers(output, headers)?;
    }
    if let Some(context) = &diagnostic.context {
        writeln!(output, "context: {context}")?;
    }
    if let Some(excerpt) = &diagnostic.excerpt {
        writeln!(output, "source: {excerpt}")?;
    }
    Ok(())
}

fn render_game_headers(output: &mut impl Write, headers: &GameHeaders) -> io::Result<()> {
    write!(output, "game headers: ")?;
    let fields = [
        ("White", headers.white.as_deref()),
        ("Black", headers.black.as_deref()),
        ("Event", headers.event.as_deref()),
        ("Date", headers.date.as_deref()),
        ("Round", headers.round.as_deref()),
    ];
    let mut separator = "";
    for (name, value) in fields {
        if let Some(value) = value {
            write!(output, "{separator}{name}={value:?}")?;
            separator = ", ";
        }
    }
    writeln!(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> impl Iterator<Item = OsString> {
        values
            .iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn parses_doctor_options_around_the_path() {
        let Action::Doctor(command) =
            parse_arguments(args(&["doctor", "--syntax-only", "games.pgn", "--lenient"])).unwrap()
        else {
            panic!("expected doctor action");
        };
        assert_eq!(command.paths, [OsString::from("games.pgn")]);
        assert_eq!(command.options.mode, ValidationMode::Syntax);
        assert!(!command.options.require_outcome);
    }

    #[test]
    fn rejects_quiet_json_output() {
        let error = parse_arguments(args(&[
            "doctor",
            "--quiet",
            "--format",
            "json",
            "games.pgn",
        ]))
        .unwrap_err();
        assert!(error.contains("cannot be combined"));
    }

    #[test]
    fn double_dash_allows_a_path_that_starts_with_a_dash() {
        let Action::Doctor(command) =
            parse_arguments(args(&["doctor", "--", "--games.pgn"])).unwrap()
        else {
            panic!("expected doctor action");
        };
        assert_eq!(command.paths, [OsString::from("--games.pgn")]);
    }

    #[test]
    fn parses_multiple_paths() {
        let Action::Doctor(command) =
            parse_arguments(args(&["doctor", "one.pgn", "two.pgn.zst"])).unwrap()
        else {
            panic!("expected doctor action");
        };
        assert_eq!(
            command.paths,
            [OsString::from("one.pgn"), OsString::from("two.pgn.zst")]
        );
    }

    #[test]
    fn rejects_stdin_among_multiple_paths() {
        let error = parse_arguments(args(&["doctor", "one.pgn", "-"])).unwrap_err();
        assert!(error.contains("standard input cannot be combined"));
    }
}
