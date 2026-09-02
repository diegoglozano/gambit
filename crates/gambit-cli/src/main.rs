mod doctor;
mod stats;

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use doctor::{
    Diagnostic, DoctorOptions, GameHeaders, Report, ReportStatus, ValidationMode, inspect,
};
use serde::Serialize;
use stats::{
    DateStats, GameLengthStats, HeaderCoverage, RatingStats, ResultCounts, StatsDiagnostic,
    StatsOptions, StatsReport, StatsStatus, inspect as inspect_stats,
};

const DEFAULT_KEEP_GOING_ERRORS: usize = 100;
const USAGE: &str = "Usage:\n  gambit doctor [OPTIONS] <PATH|->...\n  gambit stats [OPTIONS] <PATH|->...\n  gambit <PATH|->\n\nCommands:\n  doctor    Diagnose PGN syntax and chess-semantic errors\n  stats     Summarize a PGN corpus in one bounded-memory pass\n\nThe direct path form is retained as a compatibility alias for 'gambit doctor'.\nFiles ending in .zst are decompressed automatically. Directories are scanned recursively for .pgn and .pgn.zst files.\nUse - alone to read PGN from standard input.\n\nDoctor options:\n      --format <human|json|jsonl|github>  Select output format [default: human]\n      --syntax-only                       Check PGN structure without executing moves\n      --lenient                           Allow a final game without an outcome marker\n      --keep-going                        Continue after errors [default limit: 100]\n      --max-errors <N>                    Continue until N errors have been reported per input\n  -q, --quiet                             Print nothing when the input is valid\n\nStats options:\n      --format <human|json>               Select output format [default: human]\n      --lenient                           Allow a final game without an outcome marker\n\nGlobal options:\n  -h, --help                              Print help\n  -V, --version                           Print version";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutputFormat {
    Human,
    Json,
    Jsonl,
    Github,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StatsOutputFormat {
    Human,
    Json,
}

#[derive(Debug)]
struct DoctorCommand {
    paths: Vec<OsString>,
    format: OutputFormat,
    quiet: bool,
    options: DoctorOptions,
}

#[derive(Debug)]
struct StatsCommand {
    paths: Vec<OsString>,
    format: StatsOutputFormat,
    options: StatsOptions,
}

#[derive(Debug)]
enum Action {
    Help,
    Version,
    Doctor(DoctorCommand),
    Stats(StatsCommand),
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
        Ok(Action::Stats(command)) => run_stats(&command),
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
        Some("stats") => parse_stats_arguments(arguments),
        Some(value) if value.starts_with('-') && value != "-" => {
            Err(format!("unknown option or command: {value}"))
        }
        _ => {
            if arguments.next().is_some() {
                return Err(String::from(
                    "the compatibility form accepts exactly one input path; use 'gambit doctor' for options",
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
        return Err(String::from("doctor requires at least one input path"));
    }
    if paths.len() > 1 && paths.iter().any(|path| path == "-") {
        return Err(String::from(
            "standard input cannot be combined with other input paths",
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

fn parse_stats_arguments(arguments: impl Iterator<Item = OsString>) -> Result<Action, String> {
    let mut paths = Vec::new();
    let mut format = StatsOutputFormat::Human;
    let mut require_outcome = true;
    let mut arguments = arguments.peekable();

    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("-h" | "--help") => return no_more(arguments, Action::Help),
            Some("--lenient") => require_outcome = false,
            Some("--format") => {
                let value = arguments
                    .next()
                    .ok_or_else(|| String::from("--format requires a value"))?;
                format = parse_stats_format(&value)?;
            }
            Some("--") => {
                paths.extend(arguments);
                break;
            }
            Some(value) if value.starts_with("--format=") => {
                format = parse_stats_format(OsStr::new(&value["--format=".len()..]))?;
            }
            Some(value) if value.starts_with('-') && value != "-" => {
                return Err(format!("unknown stats option: {value}"));
            }
            _ => paths.push(argument),
        }
    }

    validate_input_paths("stats", &paths)?;
    Ok(Action::Stats(StatsCommand {
        paths,
        format,
        options: StatsOptions { require_outcome },
    }))
}

fn validate_input_paths(command: &str, paths: &[OsString]) -> Result<(), String> {
    if paths.is_empty() {
        return Err(format!("{command} requires at least one input path"));
    }
    if paths.len() > 1 && paths.iter().any(|path| path == "-") {
        return Err(String::from(
            "standard input cannot be combined with other input paths",
        ));
    }
    Ok(())
}

fn parse_stats_format(value: &OsStr) -> Result<StatsOutputFormat, String> {
    match value.to_str() {
        Some("human") => Ok(StatsOutputFormat::Human),
        Some("json") => Ok(StatsOutputFormat::Json),
        Some(value) => Err(format!("unknown stats output format: {value}")),
        None => Err(String::from("output format must be valid UTF-8")),
    }
}

fn parse_format(value: &std::ffi::OsStr) -> Result<OutputFormat, String> {
    match value.to_str() {
        Some("human") => Ok(OutputFormat::Human),
        Some("json") => Ok(OutputFormat::Json),
        Some("jsonl") => Ok(OutputFormat::Jsonl),
        Some("github") => Ok(OutputFormat::Github),
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
        inspect_paths(&command.paths, command.options)
    };
    let exit_code = reports.iter().map(Report::exit_code).max().unwrap_or(0);
    if let Err(error) = render_reports(&reports, command.format, command.quiet) {
        eprintln!("failed to write report: {error}");
        return ExitCode::from(3);
    }
    ExitCode::from(exit_code)
}

fn run_stats(command: &StatsCommand) -> ExitCode {
    let reports = if command.paths[0] == "-" {
        vec![inspect_stats(
            io::stdin().lock(),
            String::from("stdin"),
            command.options,
        )]
    } else {
        inspect_stats_paths(&command.paths, command.options)
    };
    let exit_code = reports
        .iter()
        .map(StatsReport::exit_code)
        .max()
        .unwrap_or(0);
    if let Err(error) = render_stats_reports(&reports, command.format) {
        eprintln!("failed to write stats: {error}");
        return ExitCode::from(3);
    }
    ExitCode::from(exit_code)
}

#[derive(Debug)]
enum DiscoveredInput {
    File(PathBuf),
    Error { source: PathBuf, message: String },
}

impl DiscoveredInput {
    fn source(&self) -> &Path {
        match self {
            Self::File(path) | Self::Error { source: path, .. } => path,
        }
    }
}

fn inspect_paths(paths: &[OsString], options: DoctorOptions) -> Vec<Report> {
    discover_inputs(paths)
        .into_iter()
        .map(|input| match input {
            DiscoveredInput::File(path) => inspect_path(path.as_os_str(), options),
            DiscoveredInput::Error { source, message } => {
                Report::input_error(source.to_string_lossy().into_owned(), options, message)
            }
        })
        .collect()
}

fn inspect_stats_paths(paths: &[OsString], options: StatsOptions) -> Vec<StatsReport> {
    discover_inputs(paths)
        .into_iter()
        .map(|input| match input {
            DiscoveredInput::File(path) => inspect_stats_path(path.as_os_str(), options),
            DiscoveredInput::Error { source, message } => {
                StatsReport::input_error(source.to_string_lossy().into_owned(), options, message)
            }
        })
        .collect()
}

fn discover_inputs(paths: &[OsString]) -> Vec<DiscoveredInput> {
    let mut discovered = Vec::new();
    for path in paths {
        let path = Path::new(path);
        if !path.is_dir() {
            discovered.push(DiscoveredInput::File(path.to_path_buf()));
            continue;
        }
        let mut inputs = discover_directory(path);
        if inputs.is_empty() {
            inputs.push(DiscoveredInput::Error {
                source: path.to_path_buf(),
                message: format!("no .pgn or .pgn.zst files found under {}", path.display()),
            });
        } else {
            inputs.sort_by(|left, right| left.source().cmp(right.source()));
        }
        discovered.extend(inputs);
    }
    discovered
}

fn discover_directory(root: &Path) -> Vec<DiscoveredInput> {
    let mut inputs = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                inputs.push(DiscoveredInput::Error {
                    source: directory.clone(),
                    message: format!("failed to read directory {}: {error}", directory.display()),
                });
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    inputs.push(DiscoveredInput::Error {
                        source: directory.clone(),
                        message: format!(
                            "failed to read an entry in {}: {error}",
                            directory.display()
                        ),
                    });
                    continue;
                }
            };
            let path = entry.path();
            match entry.file_type() {
                Ok(file_type) if file_type.is_dir() => pending.push(path),
                Ok(file_type)
                    if (file_type.is_file() || file_type.is_symlink())
                        && is_discoverable_pgn_path(&path) =>
                {
                    inputs.push(DiscoveredInput::File(path));
                }
                Ok(_) => {}
                Err(error) => inputs.push(DiscoveredInput::Error {
                    source: path.clone(),
                    message: format!("failed to inspect {}: {error}", path.display()),
                }),
            }
        }
    }
    inputs
}

fn is_discoverable_pgn_path(path: &Path) -> bool {
    if extension_is(path, "pgn") {
        return true;
    }
    extension_is(path, "zst")
        && path
            .file_stem()
            .is_some_and(|stem| extension_is(Path::new(stem), "pgn"))
}

fn extension_is(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
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

fn inspect_stats_path(path: &OsStr, options: StatsOptions) -> StatsReport {
    let source = path.to_string_lossy().into_owned();
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) => {
            return StatsReport::input_error(
                source,
                options,
                format!("failed to open {}: {error}", path.to_string_lossy()),
            );
        }
    };
    if is_zstd_path(path) {
        match zstd::stream::read::Decoder::new(file) {
            Ok(decoder) => inspect_stats(decoder, source, options),
            Err(error) => StatsReport::input_error(
                source,
                options,
                format!(
                    "failed to initialize zstd decoder for {}: {error}",
                    path.to_string_lossy()
                ),
            ),
        }
    } else {
        inspect_stats(file, source, options)
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

#[derive(Serialize)]
struct StatsBatchReport<'a> {
    schema_version: u8,
    status: StatsStatus,
    outcome_required: bool,
    input_count: usize,
    valid_input_count: usize,
    invalid_input_count: usize,
    error_input_count: usize,
    bytes: u64,
    games: u64,
    mainline_plies: u64,
    results: ResultCounts,
    game_length: GameLengthStats,
    header_coverage: HeaderCoverage,
    dates: DateStats,
    ratings: RatingStats,
    elapsed_seconds: f64,
    throughput_mib_per_second: f64,
    reports: &'a [StatsReport],
}

impl<'a> StatsBatchReport<'a> {
    fn new(reports: &'a [StatsReport]) -> Self {
        let bytes = reports.iter().map(|report| report.bytes).sum();
        let games = reports.iter().map(|report| report.games).sum();
        let mainline_plies = reports.iter().map(|report| report.mainline_plies).sum();
        let elapsed_seconds = reports
            .iter()
            .map(|report| report.elapsed_seconds)
            .sum::<f64>();
        let mut results = ResultCounts::default();
        let mut header_coverage = HeaderCoverage::default();
        let mut dates = DateStats::default();
        let mut ratings = RatingStats::default();
        let mut minimum_plies = None;
        let mut maximum_plies = None;
        for report in reports {
            results.add(report.results);
            header_coverage.add(report.header_coverage);
            dates.add(&report.dates);
            ratings.add(report.ratings);
            if let Some(value) = report.game_length.minimum_plies {
                minimum_plies =
                    Some(minimum_plies.map_or(value, |current: u64| current.min(value)));
            }
            if let Some(value) = report.game_length.maximum_plies {
                maximum_plies =
                    Some(maximum_plies.map_or(value, |current: u64| current.max(value)));
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let average_plies = if games == 0 {
            0.0
        } else {
            mainline_plies as f64 / games as f64
        };
        #[allow(clippy::cast_precision_loss)]
        let throughput_mib_per_second = if elapsed_seconds > 0.0 {
            bytes as f64 / (1024.0 * 1024.0) / elapsed_seconds
        } else {
            0.0
        };
        let status = if reports
            .iter()
            .any(|report| report.status == StatsStatus::Error)
        {
            StatsStatus::Error
        } else if reports
            .iter()
            .any(|report| report.status == StatsStatus::Invalid)
        {
            StatsStatus::Invalid
        } else {
            StatsStatus::Valid
        };
        Self {
            schema_version: 1,
            status,
            outcome_required: reports.first().is_none_or(|report| report.outcome_required),
            input_count: reports.len(),
            valid_input_count: reports
                .iter()
                .filter(|report| report.status == StatsStatus::Valid)
                .count(),
            invalid_input_count: reports
                .iter()
                .filter(|report| report.status == StatsStatus::Invalid)
                .count(),
            error_input_count: reports
                .iter()
                .filter(|report| report.status == StatsStatus::Error)
                .count(),
            bytes,
            games,
            mainline_plies,
            results,
            game_length: GameLengthStats {
                minimum_plies,
                average_plies,
                maximum_plies,
            },
            header_coverage,
            dates,
            ratings,
            elapsed_seconds,
            throughput_mib_per_second,
            reports,
        }
    }
}

fn render_stats_reports(reports: &[StatsReport], format: StatsOutputFormat) -> io::Result<()> {
    if format == StatsOutputFormat::Json {
        let mut output = io::stdout().lock();
        if let [report] = reports {
            serde_json::to_writer(&mut output, report)?;
        } else {
            serde_json::to_writer(&mut output, &StatsBatchReport::new(reports))?;
        }
        return writeln!(output);
    }

    for report in reports {
        if let Some(diagnostic) = &report.diagnostic {
            render_stats_diagnostic(report, diagnostic)?;
        }
    }
    let mut output = io::stdout().lock();
    if let [report] = reports {
        writeln!(output, "stats: {}", stats_status_label(report.status))?;
        writeln!(output, "source: {}", report.source)?;
        render_stats_metrics(
            &mut output,
            report.bytes,
            report.games,
            report.mainline_plies,
            report.results,
            report.game_length,
            report.header_coverage,
            &report.dates,
            report.ratings,
            report.elapsed_seconds,
            report.throughput_mib_per_second,
        )
    } else {
        let batch = StatsBatchReport::new(reports);
        writeln!(output, "stats: {}", stats_status_label(batch.status))?;
        writeln!(output, "inputs: {}", batch.input_count)?;
        writeln!(
            output,
            "input status: {} valid, {} invalid, {} errors",
            batch.valid_input_count, batch.invalid_input_count, batch.error_input_count
        )?;
        render_stats_metrics(
            &mut output,
            batch.bytes,
            batch.games,
            batch.mainline_plies,
            batch.results,
            batch.game_length,
            batch.header_coverage,
            &batch.dates,
            batch.ratings,
            batch.elapsed_seconds,
            batch.throughput_mib_per_second,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn render_stats_metrics(
    output: &mut impl Write,
    bytes: u64,
    games: u64,
    mainline_plies: u64,
    results: ResultCounts,
    game_length: GameLengthStats,
    header_coverage: HeaderCoverage,
    dates: &DateStats,
    ratings: RatingStats,
    elapsed_seconds: f64,
    throughput_mib_per_second: f64,
) -> io::Result<()> {
    writeln!(output, "bytes: {bytes}")?;
    writeln!(output, "games: {games}")?;
    writeln!(output, "mainline plies: {mainline_plies}")?;
    writeln!(
        output,
        "results: {} white win{}, {} black win{}, {} draw{}, {} unfinished",
        results.white_wins,
        plural_suffix_u64(results.white_wins),
        results.black_wins,
        plural_suffix_u64(results.black_wins),
        results.draws,
        plural_suffix_u64(results.draws),
        results.unfinished,
    )?;
    match (game_length.minimum_plies, game_length.maximum_plies) {
        (Some(minimum), Some(maximum)) => writeln!(
            output,
            "game length (plies): min {minimum}, avg {:.2}, max {maximum}",
            game_length.average_plies
        )?,
        _ => writeln!(output, "game length (plies): n/a")?,
    }
    writeln!(
        output,
        "header coverage: Event {}/{games}, Site {}/{games}, Date {}/{games}, Round {}/{games}, White {}/{games}, Black {}/{games}, Result {}/{games}",
        header_coverage.event,
        header_coverage.site,
        header_coverage.date,
        header_coverage.round,
        header_coverage.white,
        header_coverage.black,
        header_coverage.result,
    )?;
    if let (Some(earliest), Some(latest)) = (&dates.earliest, &dates.latest) {
        writeln!(
            output,
            "dates: {} complete ({earliest} to {latest}), {} incomplete/invalid, {} missing",
            dates.complete, dates.incomplete_or_invalid, dates.missing
        )?;
    } else {
        writeln!(
            output,
            "dates: 0 complete, {} incomplete/invalid, {} missing",
            dates.incomplete_or_invalid, dates.missing
        )?;
    }
    if let (Some(minimum), Some(maximum)) = (ratings.minimum, ratings.maximum) {
        writeln!(
            output,
            "ratings: {} numeric (min {minimum}, avg {:.2}, max {maximum}), {} invalid, {} missing",
            ratings.numeric, ratings.average, ratings.invalid, ratings.missing
        )?;
    } else {
        writeln!(
            output,
            "ratings: 0 numeric, {} invalid, {} missing",
            ratings.invalid, ratings.missing
        )?;
    }
    writeln!(output, "elapsed: {elapsed_seconds:.3}s")?;
    writeln!(output, "throughput: {throughput_mib_per_second:.2} MiB/s")
}

fn render_stats_diagnostic(report: &StatsReport, diagnostic: &StatsDiagnostic) -> io::Result<()> {
    let mut output = io::stderr().lock();
    write!(
        output,
        "{}: {}: {}",
        stats_status_label(report.status),
        report.source,
        diagnostic.category.label()
    )?;
    if diagnostic.category != stats::StatsDiagnosticCategory::Syntax {
        if let Some(byte) = diagnostic.byte {
            write!(output, " at byte {byte}")?;
        }
    }
    writeln!(output, ": {}", diagnostic.message)
}

const fn stats_status_label(status: StatsStatus) -> &'static str {
    match status {
        StatsStatus::Valid => "valid",
        StatsStatus::Invalid => "invalid",
        StatsStatus::Error => "error",
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
        OutputFormat::Github => render_github(reports),
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
        OutputFormat::Github => render_github(std::slice::from_ref(report)),
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

fn render_github(reports: &[Report]) -> io::Result<()> {
    let mut output = io::stdout().lock();
    for report in reports {
        for diagnostic in report.diagnostics() {
            write_github_annotation(&mut output, &report.source, diagnostic)?;
        }
    }

    let batch = BatchReport::new(reports);
    let limit_reached = reports.iter().any(|report| report.error_limit_reached);
    let limit_note = if limit_reached {
        "; error limit reached"
    } else {
        ""
    };
    if let [report] = reports {
        writeln!(
            output,
            "Gambit Doctor: {} - {}; {} diagnostic{}, {} complete game{}, {} move{}{}",
            status_label(report.status),
            single_line(&report.source),
            report.diagnostic_count,
            plural_suffix_usize(report.diagnostic_count),
            report.games,
            plural_suffix_u64(report.games),
            report.moves,
            plural_suffix_u64(report.moves),
            limit_note,
        )
    } else {
        writeln!(
            output,
            "Gambit Doctor: {} - {} input{}; {} diagnostic{}, {} complete game{}, {} move{}{}",
            status_label(batch.status),
            batch.input_count,
            plural_suffix_usize(batch.input_count),
            batch.diagnostic_count,
            plural_suffix_usize(batch.diagnostic_count),
            batch.games,
            plural_suffix_u64(batch.games),
            batch.moves,
            plural_suffix_u64(batch.moves),
            limit_note,
        )
    }
}

fn write_github_annotation(
    output: &mut impl Write,
    source: &str,
    diagnostic: &Diagnostic,
) -> io::Result<()> {
    write!(output, "::error ")?;
    if source != "stdin" {
        write!(output, "file={},", escape_github_property(source))?;
        if let Some(line) = diagnostic.line {
            write!(output, "line={line},")?;
            if let Some(column) = diagnostic.column {
                write!(output, "col={column},")?;
            }
        }
    }
    let title = format!(
        "Gambit Doctor: {}",
        diagnostic.category.label().replace('_', " ")
    );
    let message = github_annotation_message(diagnostic);
    writeln!(
        output,
        "title={}::{}",
        escape_github_property(&title),
        escape_github_data(&message)
    )
}

fn github_annotation_message(diagnostic: &Diagnostic) -> String {
    let mut location = Vec::new();
    if let Some(game) = diagnostic.game {
        location.push(format!("game {game}"));
    }
    if let Some(ply) = diagnostic.ply {
        location.push(format!("ply {ply}"));
    }
    let mut message = if location.is_empty() {
        diagnostic.message.clone()
    } else {
        format!("{}: {}", location.join(", "), diagnostic.message)
    };
    if let Some(context) = &diagnostic.context {
        message.push_str(" (");
        message.push_str(context);
        message.push(')');
    }
    message
}

fn escape_github_data(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn escape_github_property(value: &str) -> String {
    escape_github_data(value)
        .replace(':', "%3A")
        .replace(',', "%2C")
}

fn single_line(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

const fn status_label(status: ReportStatus) -> &'static str {
    match status {
        ReportStatus::Valid => "valid",
        ReportStatus::Invalid => "invalid",
        ReportStatus::Error => "error",
    }
}

const fn plural_suffix_usize(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

const fn plural_suffix_u64(count: u64) -> &'static str {
    if count == 1 { "" } else { "s" }
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
    fn parses_github_output_format() {
        let Action::Doctor(command) =
            parse_arguments(args(&["doctor", "--format=github", "games.pgn"])).unwrap()
        else {
            panic!("expected doctor action");
        };
        assert_eq!(command.format, OutputFormat::Github);
    }

    #[test]
    fn escapes_github_workflow_commands() {
        assert_eq!(escape_github_data("50%\r\nnext"), "50%25%0D%0Anext");
        assert_eq!(
            escape_github_property("C:\\games,2026.pgn"),
            "C%3A\\games%2C2026.pgn"
        );
    }

    #[test]
    fn rejects_stdin_among_multiple_paths() {
        let error = parse_arguments(args(&["doctor", "one.pgn", "-"])).unwrap_err();
        assert!(error.contains("standard input cannot be combined"));
    }
}
