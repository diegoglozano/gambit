mod doctor;

use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Write};
use std::process::ExitCode;

use doctor::{
    Diagnostic, DoctorOptions, GameHeaders, Report, ReportStatus, ValidationMode, inspect,
};

const USAGE: &str = "Usage:\n  gambit doctor [OPTIONS] <FILE.pgn|->\n  gambit <FILE.pgn|->\n\nCommands:\n  doctor    Diagnose PGN syntax and chess-semantic errors\n\nThe direct file form is retained as a compatibility alias for 'gambit doctor'.\nUse - to read PGN from standard input.\n\nOptions:\n      --format <human|json>  Select output format [default: human]\n      --syntax-only          Check PGN structure without executing moves\n      --lenient              Allow a final game without an outcome marker\n  -q, --quiet                Print nothing when the input is valid\n  -h, --help                 Print help\n  -V, --version              Print version";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutputFormat {
    Human,
    Json,
}

#[derive(Debug)]
struct DoctorCommand {
    path: OsString,
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
                path: first,
                format: OutputFormat::Human,
                quiet: false,
                options: DoctorOptions {
                    mode: ValidationMode::Semantic,
                    require_outcome: true,
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
    let mut path = None;
    let mut format = OutputFormat::Human;
    let mut quiet = false;
    let mut mode = ValidationMode::Semantic;
    let mut require_outcome = true;
    let mut arguments = arguments.peekable();

    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("-h" | "--help") => return no_more(arguments, Action::Help),
            Some("-q" | "--quiet") => quiet = true,
            Some("--syntax-only") => mode = ValidationMode::Syntax,
            Some("--lenient") => require_outcome = false,
            Some("--format") => {
                let value = arguments
                    .next()
                    .ok_or_else(|| String::from("--format requires a value"))?;
                format = parse_format(&value)?;
            }
            Some("--") => {
                if path.is_some() {
                    return Err(String::from("doctor accepts exactly one PGN file"));
                }
                path = Some(
                    arguments
                        .next()
                        .ok_or_else(|| String::from("-- must be followed by a PGN file"))?,
                );
                if arguments.next().is_some() {
                    return Err(String::from("doctor accepts exactly one PGN file"));
                }
            }
            Some(value) if value.starts_with("--format=") => {
                format = parse_format(std::ffi::OsStr::new(&value["--format=".len()..]))?;
            }
            Some(value) if value.starts_with('-') && value != "-" => {
                return Err(format!("unknown doctor option: {value}"));
            }
            _ if path.is_some() => return Err(String::from("doctor accepts exactly one PGN file")),
            _ => path = Some(argument),
        }
    }

    if quiet && format == OutputFormat::Json {
        return Err(String::from(
            "--quiet cannot be combined with --format json",
        ));
    }
    let path = path.ok_or_else(|| String::from("doctor requires a PGN file"))?;
    Ok(Action::Doctor(DoctorCommand {
        path,
        format,
        quiet,
        options: DoctorOptions {
            mode,
            require_outcome,
        },
    }))
}

fn parse_format(value: &std::ffi::OsStr) -> Result<OutputFormat, String> {
    match value.to_str() {
        Some("human") => Ok(OutputFormat::Human),
        Some("json") => Ok(OutputFormat::Json),
        Some(value) => Err(format!("unknown output format: {value}")),
        None => Err(String::from("output format must be valid UTF-8")),
    }
}

fn run_doctor(command: &DoctorCommand) -> ExitCode {
    let source = if command.path == "-" {
        String::from("stdin")
    } else {
        command.path.to_string_lossy().into_owned()
    };
    let report = if command.path == "-" {
        inspect(io::stdin().lock(), source, command.options)
    } else {
        match File::open(&command.path) {
            Ok(file) => inspect(file, source, command.options),
            Err(error) => Report::input_error(
                source,
                command.options,
                format!("failed to open {}: {error}", command.path.to_string_lossy()),
            ),
        }
    };
    let exit_code = report.exit_code();
    if let Err(error) = render_report(&report, command.format, command.quiet) {
        eprintln!("failed to write report: {error}");
        return ExitCode::from(3);
    }
    ExitCode::from(exit_code)
}

fn render_report(report: &Report, format: OutputFormat, quiet: bool) -> io::Result<()> {
    match format {
        OutputFormat::Json => {
            let mut output = io::stdout().lock();
            serde_json::to_writer(&mut output, report)?;
            writeln!(output)
        }
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
            if let Some(diagnostic) = &report.diagnostic {
                render_diagnostic(&mut output, diagnostic)?;
            }
            writeln!(
                output,
                "processed: {} bytes, {} complete games, {} moves",
                report.bytes, report.games, report.moves
            )
        }
    }
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
        assert_eq!(command.path, "games.pgn");
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
        assert_eq!(command.path, "--games.pgn");
    }
}
