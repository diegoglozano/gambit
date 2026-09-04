mod doctor;
mod index;
mod lichess;
mod query;
mod stats;
mod sync;

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use doctor::{
    Diagnostic, DoctorOptions, GameHeaders, Report, ReportStatus, ValidationMode, inspect,
};
use query::{
    PlayerColor, QueryError, QueryFailure, QueryFormat, QueryOptions, QuerySummary, ResultFilter,
    query as query_games,
};
use serde::Serialize;
use stats::{
    DateStats, GameLengthStats, HeaderCoverage, RatingStats, ResultCounts, StatsDiagnostic,
    StatsOptions, StatsReport, StatsStatus, TimeControlStats, inspect as inspect_stats,
};

const DEFAULT_KEEP_GOING_ERRORS: usize = 100;
const USAGE: &str = "Usage:\n  gambit doctor [OPTIONS] <PATH|->...\n  gambit stats [OPTIONS] <PATH|->...\n  gambit index [OPTIONS] --output <FILE> <PATH|->...\n  gambit query [OPTIONS] <PATH|->...\n  gambit query [OPTIONS] --lichess-user <NAME>\n  gambit sync --lichess-user <NAME> --output <DIRECTORY>\n  gambit <PATH|->\n\nCommands:\n  doctor    Diagnose PGN syntax and chess-semantic errors\n  stats     Summarize a PGN corpus in one bounded-memory pass\n  index     Build a self-contained, query-optimized .gambit database\n  query     Filter PGN or .gambit databases and emit PGN, JSONL, or a count\n  sync      Maintain a resumable local Lichess game store\n\nThe direct path form is retained as a compatibility alias for 'gambit doctor'.\nFiles ending in .zst are decompressed automatically. Directories are scanned recursively for .pgn and .pgn.zst files.\nUse - alone to read PGN from standard input.\n\nDoctor options:\n      --format <human|json|jsonl|github>  Select output format [default: human]\n      --syntax-only                       Check PGN structure without executing moves\n      --lenient                           Allow a final game without an outcome marker\n      --keep-going                        Continue after errors [default limit: 100]\n      --max-errors <N>                    Continue until N errors have been reported per input\n  -q, --quiet                             Print nothing when the input is valid\n\nStats options:\n      --format <human|json>               Select output format [default: human]\n      --lenient                           Allow a final game without an outcome marker\n\nIndex options:\n  -o, --output <FILE>                     Write the new .gambit database here\n      --format <human|json>               Select report format [default: human]\n      --update                            Update an existing database in place\n\nQuery options:\n      --lichess-user <NAME>               Stream this user's games from Lichess\n      --max-games <N>                     Limit games requested from Lichess\n      --player <NAME>                     Match a player, case-insensitively\n      --opponent <NAME>                   Match that player's opponent\n      --color <white|black>               Match the player's color\n      --result <win|loss|draw|unfinished> Match the player's result\n      --since <YYYY-MM-DD>                Match games on or after this date\n      --until <YYYY-MM-DD>                Match games on or before this date\n      --min-rating <ELO>                  Match the player's minimum rating\n      --max-rating <ELO>                  Match the player's maximum rating\n      --position <FEN>                    Match games reaching this position\n      --format <pgn|jsonl|count>          Select output format [default: pgn]\n\nSync options:\n      --lichess-user <NAME>               Select the Lichess account\n      --output <DIRECTORY>                Store one PGN file per game\n      --since <YYYY-MM-DD>                Set the first sync's earliest date\n      --format <human|json>               Select report format [default: human]\n\nGlobal options:\n  -h, --help                              Print help\n  -V, --version                           Print version";

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
struct QueryCommand {
    paths: Vec<OsString>,
    lichess_user: Option<String>,
    maximum_games: Option<u32>,
    format: QueryFormat,
    options: QueryOptions,
}

#[derive(Debug)]
struct IndexCommand {
    paths: Vec<OsString>,
    destination: PathBuf,
    format: StatsOutputFormat,
    update: bool,
}

#[derive(Debug)]
struct SyncCommand {
    username: String,
    destination: PathBuf,
    since: Option<u32>,
    format: SyncOutputFormat,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SyncOutputFormat {
    Human,
    Json,
}

#[derive(Debug)]
enum Action {
    Help,
    Version,
    Doctor(DoctorCommand),
    Stats(StatsCommand),
    Index(IndexCommand),
    Query(Box<QueryCommand>),
    Sync(SyncCommand),
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
        Ok(Action::Index(command)) => run_index(&command),
        Ok(Action::Query(command)) => run_query(&command),
        Ok(Action::Sync(command)) => run_sync(&command),
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
        Some("index") => parse_index_arguments(arguments),
        Some("query") => parse_query_arguments(arguments),
        Some("sync") => parse_sync_arguments(arguments),
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

fn parse_index_arguments(arguments: impl Iterator<Item = OsString>) -> Result<Action, String> {
    let mut paths = Vec::new();
    let mut destination = None;
    let mut format = StatsOutputFormat::Human;
    let mut update = false;
    let mut arguments = arguments.peekable();

    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("-h" | "--help") => return no_more(arguments, Action::Help),
            Some("--update") => update = true,
            Some("--") => {
                paths.extend(arguments);
                break;
            }
            Some("-o" | "--output") => {
                let value = arguments
                    .next()
                    .ok_or_else(|| String::from("--output requires a value"))?;
                if value.is_empty() || value == "-" {
                    return Err(String::from("--output must be a file path"));
                }
                destination = Some(PathBuf::from(value));
            }
            Some("--format") => {
                let value = arguments
                    .next()
                    .ok_or_else(|| String::from("--format requires a value"))?;
                format = parse_stats_format(&value)?;
            }
            Some(value) if value.starts_with("--output=") => {
                let value = OsStr::new(&value["--output=".len()..]);
                if value.is_empty() || value == "-" {
                    return Err(String::from("--output must be a file path"));
                }
                destination = Some(PathBuf::from(value));
            }
            Some(value) if value.starts_with("--format=") => {
                format = parse_stats_format(OsStr::new(&value["--format=".len()..]))?;
            }
            Some(value) if value.starts_with('-') && value != "-" => {
                return Err(format!("unknown index option: {value}"));
            }
            _ => paths.push(argument),
        }
    }

    validate_input_paths("index", &paths)?;
    if update && paths.iter().any(|path| path == "-") {
        return Err(String::from(
            "--update requires reopenable files or directories, not standard input",
        ));
    }
    Ok(Action::Index(IndexCommand {
        paths,
        destination: destination.ok_or_else(|| String::from("index requires --output"))?,
        format,
        update,
    }))
}

fn parse_query_arguments(arguments: impl Iterator<Item = OsString>) -> Result<Action, String> {
    let mut paths = Vec::new();
    let mut lichess_user = None;
    let mut maximum_games = None;
    let mut format = QueryFormat::Pgn;
    let mut options = QueryOptions::default();
    let mut arguments = arguments.peekable();

    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("-h" | "--help") => return no_more(arguments, Action::Help),
            Some("--") => {
                paths.extend(arguments);
                break;
            }
            Some(value) if value.starts_with('-') && value != "-" => {
                let (option, inline_value) = value
                    .split_once('=')
                    .map_or((value, None), |(option, value)| (option, Some(value)));
                if !is_query_value_option(option) {
                    return Err(format!("unknown query option: {option}"));
                }
                let owned_value;
                let value = if let Some(value) = inline_value {
                    OsStr::new(value)
                } else {
                    owned_value = arguments
                        .next()
                        .ok_or_else(|| format!("{option} requires a value"))?;
                    owned_value.as_os_str()
                };
                match option {
                    "--lichess-user" => {
                        lichess_user = Some(parse_lichess_username(value)?);
                    }
                    "--max-games" => maximum_games = Some(parse_maximum_games(value)?),
                    _ => set_query_option(option, value, &mut format, &mut options)?,
                }
            }
            _ => paths.push(argument),
        }
    }

    validate_query_source(&paths, lichess_user.as_deref(), maximum_games, &options)?;
    if let Some(username) = lichess_user.as_ref() {
        options.player = Some(username.clone());
    }
    validate_query_options(&options)?;
    Ok(Action::Query(Box::new(QueryCommand {
        paths,
        lichess_user,
        maximum_games,
        format,
        options,
    })))
}

fn parse_sync_arguments(arguments: impl Iterator<Item = OsString>) -> Result<Action, String> {
    let mut username = None;
    let mut destination = None;
    let mut since = None;
    let mut format = SyncOutputFormat::Human;
    let mut arguments = arguments.peekable();

    while let Some(argument) = arguments.next() {
        let Some(value) = argument.to_str() else {
            return Err(String::from("sync options must be valid UTF-8"));
        };
        if matches!(value, "-h" | "--help") {
            return no_more(arguments, Action::Help);
        }
        if !value.starts_with('-') {
            return Err(format!("unexpected sync argument: {value}"));
        }
        let (option, inline_value) = value
            .split_once('=')
            .map_or((value, None), |(option, value)| (option, Some(value)));
        if !matches!(
            option,
            "--lichess-user" | "--output" | "--since" | "--format"
        ) {
            return Err(format!("unknown sync option: {option}"));
        }
        let owned_value;
        let value = if let Some(value) = inline_value {
            OsStr::new(value)
        } else {
            owned_value = arguments
                .next()
                .ok_or_else(|| format!("{option} requires a value"))?;
            owned_value.as_os_str()
        };
        match option {
            "--lichess-user" => username = Some(parse_lichess_username(value)?),
            "--output" => {
                if value.is_empty() {
                    return Err(String::from("--output must not be empty"));
                }
                destination = Some(PathBuf::from(value));
            }
            "--since" => since = Some(parse_query_date(option, value)?),
            "--format" => format = parse_sync_format(value)?,
            _ => unreachable!("sync option was checked before dispatch"),
        }
    }

    Ok(Action::Sync(SyncCommand {
        username: username.ok_or_else(|| String::from("sync requires --lichess-user"))?,
        destination: destination.ok_or_else(|| String::from("sync requires --output"))?,
        since,
        format,
    }))
}

fn parse_sync_format(value: &OsStr) -> Result<SyncOutputFormat, String> {
    match value.to_str() {
        Some("human") => Ok(SyncOutputFormat::Human),
        Some("json") => Ok(SyncOutputFormat::Json),
        Some(value) => Err(format!("unknown sync output format: {value}")),
        None => Err(String::from("sync output format must be valid UTF-8")),
    }
}

fn is_query_value_option(option: &str) -> bool {
    matches!(
        option,
        "--lichess-user"
            | "--max-games"
            | "--player"
            | "--opponent"
            | "--color"
            | "--result"
            | "--since"
            | "--until"
            | "--min-rating"
            | "--max-rating"
            | "--position"
            | "--format"
    )
}

fn validate_query_source(
    paths: &[OsString],
    lichess_user: Option<&str>,
    maximum_games: Option<u32>,
    options: &QueryOptions,
) -> Result<(), String> {
    match (paths.is_empty(), lichess_user) {
        (true, None) => {
            return Err(String::from(
                "query requires an input path or --lichess-user",
            ));
        }
        (false, Some(_)) => {
            return Err(String::from(
                "--lichess-user cannot be combined with input paths",
            ));
        }
        _ => {}
    }
    if lichess_user.is_some() && options.player.is_some() {
        return Err(String::from(
            "--lichess-user selects the player and cannot be combined with --player",
        ));
    }
    if maximum_games.is_some() && lichess_user.is_none() {
        return Err(String::from("--max-games requires --lichess-user"));
    }
    if paths.len() > 1 && paths.iter().any(|path| path == "-") {
        return Err(String::from(
            "standard input cannot be combined with other input paths",
        ));
    }
    Ok(())
}

fn parse_lichess_username(value: &OsStr) -> Result<String, String> {
    let username = parse_query_text("--lichess-user", value)?;
    if !(2..=30).contains(&username.len())
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(String::from(
            "--lichess-user must contain 2-30 letters, digits, underscores, or hyphens",
        ));
    }
    Ok(username)
}

fn parse_maximum_games(value: &OsStr) -> Result<u32, String> {
    let value = value
        .to_str()
        .ok_or_else(|| String::from("--max-games must be valid UTF-8"))?;
    match value.parse() {
        Ok(0) | Err(_) => Err(String::from("--max-games must be a positive integer")),
        Ok(maximum) => Ok(maximum),
    }
}

fn set_query_option(
    option: &str,
    value: &OsStr,
    format: &mut QueryFormat,
    options: &mut QueryOptions,
) -> Result<(), String> {
    match option {
        "--player" => options.player = Some(parse_query_text(option, value)?),
        "--opponent" => options.opponent = Some(parse_query_text(option, value)?),
        "--color" => options.color = Some(parse_query_color(value)?),
        "--result" => options.result = Some(parse_query_result(value)?),
        "--since" => options.since = Some(parse_query_date(option, value)?),
        "--until" => options.until = Some(parse_query_date(option, value)?),
        "--min-rating" => options.minimum_rating = Some(parse_query_rating(option, value)?),
        "--max-rating" => options.maximum_rating = Some(parse_query_rating(option, value)?),
        "--position" => options.position = Some(parse_query_position(option, value)?),
        "--format" => *format = parse_query_format(value)?,
        _ => unreachable!("query option was checked before dispatch"),
    }
    Ok(())
}

fn validate_query_options(options: &QueryOptions) -> Result<(), String> {
    if options.player.is_none()
        && (options.opponent.is_some()
            || options.color.is_some()
            || options.minimum_rating.is_some()
            || options.maximum_rating.is_some()
            || matches!(options.result, Some(ResultFilter::Win | ResultFilter::Loss)))
    {
        return Err(String::from(
            "--opponent, --color, rating bounds, and win/loss results require --player",
        ));
    }
    if options
        .since
        .zip(options.until)
        .is_some_and(|(since, until)| since > until)
    {
        return Err(String::from("--since must not be later than --until"));
    }
    if options
        .minimum_rating
        .zip(options.maximum_rating)
        .is_some_and(|(minimum, maximum)| minimum > maximum)
    {
        return Err(String::from("--min-rating must not exceed --max-rating"));
    }
    Ok(())
}

fn parse_query_text(option: &str, value: &OsStr) -> Result<String, String> {
    let value = value
        .to_str()
        .ok_or_else(|| format!("{option} must be valid UTF-8"))?;
    if value.is_empty() {
        Err(format!("{option} must not be empty"))
    } else {
        Ok(String::from(value))
    }
}

fn parse_query_color(value: &OsStr) -> Result<PlayerColor, String> {
    match value.to_str() {
        Some("white") => Ok(PlayerColor::White),
        Some("black") => Ok(PlayerColor::Black),
        Some(value) => Err(format!("unknown query color: {value}")),
        None => Err(String::from("query color must be valid UTF-8")),
    }
}

fn parse_query_result(value: &OsStr) -> Result<ResultFilter, String> {
    match value.to_str() {
        Some("win") => Ok(ResultFilter::Win),
        Some("loss") => Ok(ResultFilter::Loss),
        Some("draw") => Ok(ResultFilter::Draw),
        Some("unfinished") => Ok(ResultFilter::Unfinished),
        Some(value) => Err(format!("unknown query result: {value}")),
        None => Err(String::from("query result must be valid UTF-8")),
    }
}

fn parse_query_date(option: &str, value: &OsStr) -> Result<u32, String> {
    let value = value
        .to_str()
        .ok_or_else(|| format!("{option} must be valid UTF-8"))?;
    query::parse_date(value)
        .ok_or_else(|| format!("{option} must be a real date in YYYY-MM-DD format"))
}

fn parse_query_rating(option: &str, value: &OsStr) -> Result<u32, String> {
    let value = value
        .to_str()
        .ok_or_else(|| format!("{option} must be valid UTF-8"))?;
    value
        .parse()
        .map_err(|_| format!("{option} must be an unsigned integer"))
}

fn parse_query_position(option: &str, value: &OsStr) -> Result<gambit_chess::Position, String> {
    let value = value
        .to_str()
        .ok_or_else(|| format!("{option} must be valid UTF-8"))?;
    gambit_chess::Position::from_fen(value.as_bytes())
        .map_err(|error| format!("{option} must be a valid six-field FEN: {error}"))
}

fn parse_query_format(value: &OsStr) -> Result<QueryFormat, String> {
    match value.to_str() {
        Some("pgn") => Ok(QueryFormat::Pgn),
        Some("jsonl") => Ok(QueryFormat::Jsonl),
        Some("count") => Ok(QueryFormat::Count),
        Some(value) => Err(format!("unknown query output format: {value}")),
        None => Err(String::from("query output format must be valid UTF-8")),
    }
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

fn run_index(command: &IndexCommand) -> ExitCode {
    let started = Instant::now();
    let summary = if command.update {
        update_index(command)
    } else {
        build_index(command)
    };
    let summary = match summary {
        Ok(summary) => summary,
        Err(error) => {
            eprintln!("index: {error}");
            return ExitCode::from(error.exit_code());
        }
    };
    let report = IndexReport::new(
        &command.destination,
        &summary,
        started.elapsed().as_secs_f64(),
        if command.update { "update" } else { "build" },
    );
    if let Err(error) = render_index_report(&report, command.format) {
        eprintln!("failed to write index report: {error}");
        return ExitCode::from(3);
    }
    ExitCode::SUCCESS
}

fn build_index(command: &IndexCommand) -> Result<index::IndexSummary, index::IndexError> {
    let mut builder = index::Builder::create(&command.destination)?;

    if command.paths[0] == "-" {
        builder.add(io::stdin().lock(), "stdin")?;
    } else {
        for input in discover_inputs(&command.paths) {
            match input {
                DiscoveredInput::File(path) => add_index_path(&mut builder, &path),
                DiscoveredInput::Error { source, message } => {
                    return Err(index::IndexError::Io {
                        context: source.display().to_string(),
                        error: io::Error::other(message),
                    });
                }
            }?;
        }
    }
    builder.finish()
}

fn update_index(command: &IndexCommand) -> Result<index::IndexSummary, index::IndexError> {
    let mut updater = index::Updater::open(&command.destination)?;
    for input in discover_inputs(&command.paths) {
        let path = match input {
            DiscoveredInput::File(path) => path,
            DiscoveredInput::Error { source, message } => {
                return Err(index::IndexError::Io {
                    context: source.display().to_string(),
                    error: io::Error::other(message),
                });
            }
        };
        let source = path.to_string_lossy().into_owned();
        let reader = open_index_path(&path)?;
        let fingerprint = index::fingerprint(reader, &source)?;
        if updater.prepare(&source, &fingerprint)? == index::UpdateAction::Write {
            updater.add(open_index_path(&path)?, &source, &fingerprint)?;
        }
    }
    updater.finish()
}

fn add_index_path(builder: &mut index::Builder, path: &Path) -> Result<(), index::IndexError> {
    let source = path.to_string_lossy().into_owned();
    builder.add(open_index_path(path)?, &source)
}

fn open_index_path(path: &Path) -> Result<Box<dyn io::Read>, index::IndexError> {
    let source = path.to_string_lossy().into_owned();
    let file = File::open(path).map_err(|error| index::IndexError::Io {
        context: format!("failed to open {source}"),
        error,
    })?;
    if is_zstd_path(path.as_os_str()) {
        let decoder =
            zstd::stream::read::Decoder::new(file).map_err(|error| index::IndexError::Io {
                context: format!("failed to initialize zstd decoder for {source}"),
                error,
            })?;
        Ok(Box::new(decoder))
    } else {
        Ok(Box::new(file))
    }
}

#[derive(Serialize)]
struct IndexReport {
    schema_version: u32,
    status: &'static str,
    mode: &'static str,
    destination: String,
    sources: u64,
    skipped_sources: u64,
    replaced_sources: u64,
    games: u64,
    positions: u64,
    pgn_bytes: u64,
    scanned_pgn_bytes: u64,
    database_bytes: u64,
    elapsed_seconds: f64,
    throughput_mib_per_second: f64,
}

impl IndexReport {
    fn new(
        destination: &Path,
        summary: &index::IndexSummary,
        elapsed_seconds: f64,
        mode: &'static str,
    ) -> Self {
        #[allow(clippy::cast_precision_loss)]
        let throughput_mib_per_second = if elapsed_seconds > 0.0 {
            summary.scanned_pgn_bytes as f64 / (1024.0 * 1024.0) / elapsed_seconds
        } else {
            0.0
        };
        Self {
            schema_version: summary.schema_version,
            status: "complete",
            mode,
            destination: destination.to_string_lossy().into_owned(),
            sources: summary.sources,
            skipped_sources: summary.skipped_sources,
            replaced_sources: summary.replaced_sources,
            games: summary.games,
            positions: summary.positions,
            pgn_bytes: summary.pgn_bytes,
            scanned_pgn_bytes: summary.scanned_pgn_bytes,
            database_bytes: summary.database_bytes,
            elapsed_seconds,
            throughput_mib_per_second,
        }
    }
}

fn render_index_report(report: &IndexReport, format: StatsOutputFormat) -> io::Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    match format {
        StatsOutputFormat::Human => {
            writeln!(output, "index: {}", report.status)?;
            writeln!(output, "mode: {}", report.mode)?;
            writeln!(output, "destination: {}", report.destination)?;
            writeln!(output, "sources written: {}", report.sources)?;
            writeln!(output, "sources skipped: {}", report.skipped_sources)?;
            writeln!(output, "sources replaced: {}", report.replaced_sources)?;
            writeln!(output, "games written: {}", report.games)?;
            writeln!(output, "positions written: {}", report.positions)?;
            writeln!(
                output,
                "source PGN bytes scanned: {}",
                report.scanned_pgn_bytes
            )?;
            writeln!(output, "source PGN bytes written: {}", report.pgn_bytes)?;
            writeln!(output, "database bytes: {}", report.database_bytes)?;
            writeln!(output, "elapsed: {:.3}s", report.elapsed_seconds)?;
            writeln!(
                output,
                "throughput: {:.2} MiB/s",
                report.throughput_mib_per_second
            )
        }
        StatsOutputFormat::Json => {
            serde_json::to_writer_pretty(&mut output, report).map_err(io::Error::other)?;
            writeln!(output)
        }
    }
}

fn run_query(command: &QueryCommand) -> ExitCode {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut total = QuerySummary::default();
    let mut exit_code = 0;

    if let Some(username) = command.lichess_user.as_deref() {
        let token = match lichess_token() {
            Ok(token) => token,
            Err(error) => {
                eprintln!("query: {error}");
                return ExitCode::from(3);
            }
        };
        let request = lichess::UserGamesRequest {
            username,
            maximum_games: command.maximum_games,
            options: &command.options,
            since_timestamp: None,
            until_timestamp: None,
            include_ongoing: false,
            oldest_first: false,
        };
        let mut response = match lichess::user_games(&request, token.as_deref()) {
            Ok(response) => response,
            Err(error) => {
                eprintln!("query: lichess:{username}: {error}");
                return ExitCode::from(3);
            }
        };
        let source = format!("lichess:{username}");
        match query_games(
            response.body_mut().as_reader(),
            &source,
            &command.options,
            command.format,
            &mut output,
        ) {
            Ok(summary) => total.add(summary),
            Err(failure) => {
                total.add(failure.summary);
                eprintln!("query: {source}: {}", failure.error);
                if matches!(failure.error, QueryError::Output(_)) {
                    return ExitCode::from(3);
                }
                exit_code = failure.error.exit_code();
            }
        }
    } else if command.paths[0] == "-" {
        match query_games(
            io::stdin().lock(),
            "stdin",
            &command.options,
            command.format,
            &mut output,
        ) {
            Ok(summary) => total.add(summary),
            Err(failure) => {
                total.add(failure.summary);
                eprintln!("query: stdin: {}", failure.error);
                if matches!(failure.error, QueryError::Output(_)) {
                    return ExitCode::from(3);
                }
                exit_code = failure.error.exit_code();
            }
        }
    } else {
        for input in discover_inputs(&command.paths) {
            let result = match input {
                DiscoveredInput::File(path) => query_path(
                    path.as_os_str(),
                    &command.options,
                    command.format,
                    &mut output,
                ),
                DiscoveredInput::Error { source, message } => {
                    eprintln!("query: {}: {message}", source.display());
                    exit_code = exit_code.max(3);
                    continue;
                }
            };
            match result {
                Ok(summary) => total.add(summary),
                Err((source, failure)) => {
                    total.add(failure.summary);
                    eprintln!("query: {source}: {}", failure.error);
                    if matches!(failure.error, QueryError::Output(_)) {
                        return ExitCode::from(3);
                    }
                    exit_code = exit_code.max(failure.error.exit_code());
                }
            }
        }
    }

    if command.format == QueryFormat::Count
        && exit_code == 0
        && writeln!(output, "{}", total.matches).is_err()
    {
        eprintln!("failed to write query count");
        return ExitCode::from(3);
    }
    ExitCode::from(exit_code)
}

fn run_sync(command: &SyncCommand) -> ExitCode {
    let (token, now_milliseconds) = match sync_runtime_context() {
        Ok(context) => context,
        Err(error) => {
            eprintln!("sync: {error}");
            return ExitCode::from(3);
        }
    };
    let plan = match sync::prepare(
        &command.destination,
        &command.username,
        now_milliseconds,
        command.since,
    ) {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("sync: {}: {error}", command.destination.display());
            return ExitCode::from(3);
        }
    };
    if command.since.is_some() && !plan.is_initial() {
        eprintln!("sync: --since can only initialize a new sync destination");
        return ExitCode::from(2);
    }

    let options = QueryOptions {
        since: if plan.is_initial() {
            plan.initial_since
        } else {
            None
        },
        ..QueryOptions::default()
    };
    let request = lichess::UserGamesRequest {
        username: &command.username,
        maximum_games: None,
        options: &options,
        since_timestamp: plan.since_timestamp,
        until_timestamp: Some(plan.until_timestamp),
        include_ongoing: true,
        oldest_first: true,
    };
    let mut response = match lichess::user_games(&request, token.as_deref()) {
        Ok(response) => response,
        Err(error) => {
            eprintln!("sync: lichess:{}: {error}", command.username);
            return ExitCode::from(3);
        }
    };
    if let Err(error) = sync::start(&plan) {
        eprintln!("sync: {}: {error}", command.destination.display());
        return ExitCode::from(3);
    }
    let mut summary = match sync::ingest(response.body_mut().as_reader(), &plan, None) {
        Ok(summary) => summary,
        Err(error) => {
            eprintln!("sync: lichess:{}: {error}", command.username);
            return ExitCode::from(3);
        }
    };

    let refreshed_unfinished = plan.unfinished_game_ids.len();
    for game_id in &plan.unfinished_game_ids {
        let mut response = match lichess::game(game_id, token.as_deref()) {
            Ok(response) => response,
            Err(lichess::LichessError::GameNotFound(_)) => {
                eprintln!("sync: warning: unfinished game {game_id} no longer exists on Lichess");
                summary.statuses.push(sync::GameStatus {
                    game_id: game_id.clone(),
                    unfinished: false,
                });
                continue;
            }
            Err(error) => {
                eprintln!("sync: lichess:{game_id}: {error}");
                return ExitCode::from(3);
            }
        };
        match sync::ingest(response.body_mut().as_reader(), &plan, Some(game_id)) {
            Ok(refresh) => summary.add(refresh),
            Err(error) => {
                eprintln!("sync: lichess:{game_id}: {error}");
                return ExitCode::from(3);
            }
        }
    }
    if let Err(error) = finish_sync(command, &plan, &mut summary, refreshed_unfinished) {
        eprintln!("sync: {error}");
        return ExitCode::from(3);
    }
    ExitCode::SUCCESS
}

fn finish_sync(
    command: &SyncCommand,
    plan: &sync::SyncPlan,
    summary: &mut sync::IngestSummary,
    refreshed_unfinished: usize,
) -> Result<(), String> {
    let statuses = std::mem::take(&mut summary.statuses);
    let unfinished = sync::finish(plan, statuses)
        .map_err(|error| format!("{}: {error}", command.destination.display()))?;
    let report = SyncReport {
        schema_version: 1,
        status: "complete",
        source: format!("lichess:{}", command.username),
        destination: command.destination.to_string_lossy().into_owned(),
        received: summary.received,
        created: summary.created,
        updated: summary.updated,
        unchanged: summary.unchanged,
        refreshed_unfinished,
        unfinished,
        cursor_milliseconds: plan.until_timestamp,
    };
    render_sync_report(&report, command.format)
        .map_err(|error| format!("failed to write report: {error}"))
}

fn lichess_token() -> Result<Option<String>, &'static str> {
    match env::var("LICHESS_TOKEN") {
        Ok(token) if token.is_empty() => Ok(None),
        Ok(token) => Ok(Some(token)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err("LICHESS_TOKEN must be valid UTF-8"),
    }
}

fn current_time_milliseconds() -> Result<i64, String> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before the Unix epoch: {error}"))?;
    i64::try_from(duration.as_millis())
        .map_err(|_| String::from("system time is outside the supported range"))
}

fn sync_runtime_context() -> Result<(Option<String>, i64), String> {
    let token = lichess_token().map_err(String::from)?;
    let now_milliseconds = current_time_milliseconds()?;
    Ok((token, now_milliseconds))
}

#[derive(Serialize)]
struct SyncReport {
    schema_version: u8,
    status: &'static str,
    source: String,
    destination: String,
    received: u64,
    created: u64,
    updated: u64,
    unchanged: u64,
    refreshed_unfinished: usize,
    unfinished: usize,
    cursor_milliseconds: i64,
}

fn render_sync_report(report: &SyncReport, format: SyncOutputFormat) -> io::Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    match format {
        SyncOutputFormat::Human => {
            writeln!(output, "sync: {}", report.status)?;
            writeln!(output, "source: {}", report.source)?;
            writeln!(output, "destination: {}", report.destination)?;
            writeln!(output, "received: {}", report.received)?;
            writeln!(output, "created: {}", report.created)?;
            writeln!(output, "updated: {}", report.updated)?;
            writeln!(output, "unchanged: {}", report.unchanged)?;
            writeln!(
                output,
                "unfinished: {} ({} refreshed)",
                report.unfinished, report.refreshed_unfinished
            )?;
            writeln!(output, "cursor: {} ms", report.cursor_milliseconds)
        }
        SyncOutputFormat::Json => {
            serde_json::to_writer_pretty(&mut output, report).map_err(io::Error::other)?;
            writeln!(output)
        }
    }
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

fn query_path(
    path: &OsStr,
    options: &QueryOptions,
    format: QueryFormat,
    output: &mut impl Write,
) -> Result<QuerySummary, (String, QueryFailure)> {
    let source = path.to_string_lossy().into_owned();
    if extension_is(Path::new(path), "gambit") {
        return index::query(Path::new(path), options, format, output)
            .map_err(|error| (source, error));
    }
    let file = File::open(path).map_err(|error| {
        (
            source.clone(),
            QueryFailure {
                summary: QuerySummary::default(),
                error: QueryError::Frame(gambit_pgn::FrameError::Io(error)),
            },
        )
    })?;
    let result = if is_zstd_path(path) {
        let decoder = zstd::stream::read::Decoder::new(file).map_err(|error| {
            (
                source.clone(),
                QueryFailure {
                    summary: QuerySummary::default(),
                    error: QueryError::Frame(gambit_pgn::FrameError::Io(error)),
                },
            )
        })?;
        query_games(decoder, &source, options, format, output)
    } else {
        query_games(file, &source, options, format, output)
    };
    result.map_err(|error| (source, error))
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
    time_controls: TimeControlStats,
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
        let mut game_length_distribution = stats::GameLengthDistribution::default();
        let mut time_controls = TimeControlStats::default();
        let mut minimum_plies = None;
        let mut maximum_plies = None;
        for report in reports {
            results.add(report.results);
            header_coverage.add(report.header_coverage);
            dates.add(&report.dates);
            ratings.add(report.ratings);
            game_length_distribution.add(report.game_length.distribution);
            time_controls.add(report.time_controls);
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
                distribution: game_length_distribution,
            },
            header_coverage,
            dates,
            ratings,
            time_controls,
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
            report.time_controls,
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
            batch.time_controls,
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
    time_controls: TimeControlStats,
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
        "game-length buckets: 0={}, 1-20={}, 21-40={}, 41-60={}, 61-80={}, 81-120={}, 121-160={}, 161+={}",
        game_length.distribution.zero,
        game_length.distribution.from_1_to_20,
        game_length.distribution.from_21_to_40,
        game_length.distribution.from_41_to_60,
        game_length.distribution.from_61_to_80,
        game_length.distribution.from_81_to_120,
        game_length.distribution.from_121_to_160,
        game_length.distribution.at_least_161,
    )?;
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
    writeln!(
        output,
        "rating bands: <1000={}, 1000-1199={}, 1200-1399={}, 1400-1599={}, 1600-1799={}, 1800-1999={}, 2000-2199={}, 2200-2399={}, 2400+={}",
        ratings.distribution.under_1000,
        ratings.distribution.from_1000_to_1199,
        ratings.distribution.from_1200_to_1399,
        ratings.distribution.from_1400_to_1599,
        ratings.distribution.from_1600_to_1799,
        ratings.distribution.from_1800_to_1999,
        ratings.distribution.from_2000_to_2199,
        ratings.distribution.from_2200_to_2399,
        ratings.distribution.at_least_2400,
    )?;
    writeln!(
        output,
        "time controls: sudden death={}, increment={}, moves/period={}, multi-stage={}, hourglass={}, unknown={}, unlimited={}, invalid={}, missing={}",
        time_controls.sudden_death,
        time_controls.increment,
        time_controls.moves_per_period,
        time_controls.multi_stage,
        time_controls.hourglass,
        time_controls.unknown,
        time_controls.unlimited,
        time_controls.invalid,
        time_controls.missing,
    )?;
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

    #[test]
    fn parses_lichess_as_a_query_source_and_selects_that_player() {
        let Action::Query(command) = parse_arguments(args(&[
            "query",
            "--lichess-user",
            "diegoglozano",
            "--max-games=25",
            "--color",
            "black",
            "--result",
            "loss",
        ]))
        .unwrap() else {
            panic!("expected query action");
        };
        assert!(command.paths.is_empty());
        assert_eq!(command.lichess_user.as_deref(), Some("diegoglozano"));
        assert_eq!(command.maximum_games, Some(25));
        assert_eq!(command.options.player.as_deref(), Some("diegoglozano"));
        assert_eq!(command.options.color, Some(PlayerColor::Black));
        assert_eq!(command.options.result, Some(ResultFilter::Loss));
    }

    #[test]
    fn rejects_ambiguous_or_inapplicable_lichess_source_options() {
        let mixed = parse_arguments(args(&[
            "query",
            "--lichess-user",
            "diegoglozano",
            "games.pgn",
        ]))
        .unwrap_err();
        assert!(mixed.contains("cannot be combined with input paths"));

        let player = parse_arguments(args(&[
            "query",
            "--lichess-user",
            "diegoglozano",
            "--player",
            "someone",
        ]))
        .unwrap_err();
        assert!(player.contains("selects the player"));

        let maximum =
            parse_arguments(args(&["query", "--max-games", "10", "games.pgn"])).unwrap_err();
        assert!(maximum.contains("requires --lichess-user"));
    }

    #[test]
    fn parses_a_lichess_sync_command() {
        let Action::Sync(command) = parse_arguments(args(&[
            "sync",
            "--lichess-user=diegoglozano",
            "--output",
            "my-games",
            "--since",
            "2026-01-01",
            "--format=json",
        ]))
        .unwrap() else {
            panic!("expected sync action");
        };
        assert_eq!(command.username, "diegoglozano");
        assert_eq!(command.destination, PathBuf::from("my-games"));
        assert_eq!(command.since, Some(20_260_101));
        assert_eq!(command.format, SyncOutputFormat::Json);
    }

    #[test]
    fn parses_an_index_command() {
        let Action::Index(command) = parse_arguments(args(&[
            "index",
            "games.pgn.zst",
            "--output=archive.gambit",
            "--format",
            "json",
        ]))
        .unwrap() else {
            panic!("expected index action");
        };
        assert_eq!(command.paths, [OsString::from("games.pgn.zst")]);
        assert_eq!(command.destination, PathBuf::from("archive.gambit"));
        assert_eq!(command.format, StatsOutputFormat::Json);
        assert!(!command.update);
    }

    #[test]
    fn parses_index_update_and_rejects_stdin() {
        let Action::Index(command) = parse_arguments(args(&[
            "index",
            "--update",
            "games",
            "--output=archive.gambit",
        ]))
        .unwrap() else {
            panic!("expected index action");
        };
        assert!(command.update);

        let error = parse_arguments(args(&["index", "--update", "--output=archive.gambit", "-"]))
            .unwrap_err();
        assert!(error.contains("not standard input"));
    }

    #[test]
    fn sync_requires_a_user_and_destination() {
        let missing_user = parse_arguments(args(&["sync", "--output", "my-games"])).unwrap_err();
        assert!(missing_user.contains("requires --lichess-user"));
        let missing_output =
            parse_arguments(args(&["sync", "--lichess-user", "diegoglozano"])).unwrap_err();
        assert!(missing_output.contains("requires --output"));
    }
}
