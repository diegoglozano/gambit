//! Local, synchronous UCI boundary. Call from a background worker, never the UI thread.
//! Each request owns one process; dropping it always reaps the child. Callers must
//! serialize requests. No cache or product diagnosis is implied by an engine score.

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant};

mod position;
pub use position::GamePosition;

const MAX_LINE: usize = 8192;
const MAX_PV: usize = 32;
const MAX_EVIDENCE: usize = 64;
const POLL: Duration = Duration::from_millis(20);
/// Version the interpretation of UCI evidence independently of Stockfish's build.
pub const EVIDENCE_VERSION: u32 = 2;

#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Score {
    Centipawns(i32),
    Mate(i32),
}

impl Score {
    /// UCI scores are relative to the root side to move, including mate distance.
    #[must_use]
    pub fn for_player(self, player_is_side_to_move: bool) -> Self {
        if player_is_side_to_move {
            return self;
        }
        match self {
            Self::Centipawns(n) => Self::Centipawns(n.saturating_neg()),
            Self::Mate(n) => Self::Mate(n.saturating_neg()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Analysis {
    pub engine: String,
    pub nodes_budget: u64,
    pub score: Score,
    /// Bounds must not be treated as exact evaluations in diagnosis.
    pub score_bound: ScoreBound,
    pub score_source: ScoreSource,
    pub depth: Option<u32>,
    pub best_move: Option<String>,
    pub pv: Vec<String>,
    /// Legally replayed standard notation, in the same order as `pv`.
    pub pv_san: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreBound {
    Exact,
    Lower,
    Upper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreSource {
    LatestReport,
    PreviousCompletedIteration,
}

impl ScoreBound {
    /// Negating a player-relative score also reverses its inequality.
    #[must_use]
    pub fn for_player(self, player_is_side_to_move: bool) -> Self {
        if player_is_side_to_move {
            return self;
        }
        match self {
            Self::Exact => Self::Exact,
            Self::Lower => Self::Upper,
            Self::Upper => Self::Lower,
        }
    }
}

#[derive(Debug, PartialEq)]
struct Info {
    score: Score,
    bound: ScoreBound,
    depth: Option<u32>,
    pv: Vec<String>,
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Protocol(&'static str),
    Cancelled,
    Timeout,
    Exited,
    InvalidPosition,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

fn uci_move(s: &str) -> bool {
    let b = s.as_bytes();
    (b.len() == 4 || b.len() == 5)
        && (b'a'..=b'h').contains(&b[0])
        && (b'1'..=b'8').contains(&b[1])
        && (b'a'..=b'h').contains(&b[2])
        && (b'1'..=b'8').contains(&b[3])
        && (b.len() == 4 || b"qrbn".contains(&b[4]))
}

/// Preserve score bounds; ignore secondary variations and diagnostic strings.
fn parse_info(line: &str) -> Result<Option<Info>, Error> {
    if line.len() > MAX_LINE {
        return Err(Error::Protocol("oversized line"));
    }
    let words: Vec<_> = line.split_whitespace().collect();
    if words.first() != Some(&"info") || words.get(1) == Some(&"string") {
        return Ok(None);
    }
    if let Some(i) = words.iter().position(|w| *w == "multipv") {
        if words.get(i + 1) != Some(&"1") {
            return Ok(None);
        }
    }
    let Some(i) = words.iter().position(|w| *w == "score") else {
        return Ok(None);
    };
    let value = words
        .get(i + 2)
        .and_then(|n| n.parse::<i32>().ok())
        .filter(|n| *n != i32::MIN)
        .ok_or(Error::Protocol("invalid score"))?;
    let score = match words.get(i + 1) {
        Some(&"cp") => Score::Centipawns(value),
        Some(&"mate") => Score::Mate(value),
        _ => return Err(Error::Protocol("invalid score type")),
    };
    let bound = match (words.contains(&"lowerbound"), words.contains(&"upperbound")) {
        (false, false) => ScoreBound::Exact,
        (true, false) => ScoreBound::Lower,
        (false, true) => ScoreBound::Upper,
        (true, true) => return Err(Error::Protocol("conflicting score bounds")),
    };
    let depth = words
        .iter()
        .position(|w| *w == "depth")
        .map(|i| {
            words
                .get(i + 1)
                .and_then(|s| s.parse().ok())
                .ok_or(Error::Protocol("invalid depth"))
        })
        .transpose()?;
    let mut pv = Vec::new();
    if let Some(i) = words.iter().position(|w| *w == "pv") {
        for word in words.iter().skip(i + 1).take(MAX_PV) {
            if !uci_move(word) {
                return Err(Error::Protocol("invalid variation"));
            }
            pv.push((*word).to_owned());
        }
    }
    Ok(Some(Info {
        score,
        bound,
        depth,
        pv,
    }))
}

fn read_line(reader: &mut impl BufRead) -> Result<Option<String>, Error> {
    let mut bytes = Vec::new();
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Err(Error::Protocol("truncated line"))
            };
        }
        let end = chunk.iter().position(|b| *b == b'\n');
        let count = end.map_or(chunk.len(), |i| i + 1);
        if bytes.len() + count > MAX_LINE {
            return Err(Error::Protocol("oversized line"));
        }
        bytes.extend_from_slice(&chunk[..count]);
        reader.consume(count);
        if end.is_some() {
            return String::from_utf8(bytes)
                .map(|s| Some(s.trim_end().to_owned()))
                .map_err(|_| Error::Protocol("invalid UTF-8"));
        }
    }
}

struct Process {
    child: Child,
    stdin: ChildStdin,
    lines: Option<mpsc::Receiver<Result<String, Error>>>,
    reader: Option<thread::JoinHandle<()>>,
}

impl Process {
    fn spawn(path: &Path) -> Result<Self, Error> {
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, rx) = mpsc::sync_channel(64);
        let reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let item = match read_line(&mut reader) {
                    Ok(Some(s)) => Ok(s),
                    Ok(None) => break,
                    Err(e) => Err(e),
                };
                let failed = item.is_err();
                if tx.send(item).is_err() || failed {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            lines: Some(rx),
            reader: Some(reader),
        })
    }

    fn send(&mut self, command: &str) -> Result<(), Error> {
        writeln!(self.stdin, "{command}")?;
        self.stdin.flush()?;
        Ok(())
    }

    fn next(&self, cancel: &Cancellation, deadline: Instant) -> Result<String, Error> {
        loop {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            match self
                .lines
                .as_ref()
                .expect("live receiver")
                .recv_timeout(POLL)
            {
                Ok(line) => return line,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(Error::Exited),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }

    fn until(&self, token: &str, cancel: &Cancellation, deadline: Instant) -> Result<(), Error> {
        while self.next(cancel, deadline)? != token {}
        Ok(())
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // Small commands fit in the pipe even when the engine stopped reading.
        let _ = self.send("stop");
        let _ = self.send("quit");
        let deadline = Instant::now() + Duration::from_millis(250);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.lines.take(); // Unblock a reader waiting on the bounded channel.
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

/// Analyze a standard FEN with one thread, 16 MiB hash and a fresh process.
/// `timeout` is a watchdog, not the search budget. Cancellation kills unfinished
/// work; no partial result is returned. Terminal positions retain mate/cp type.
///
/// # Errors
/// Returns an error on invalid input, cancellation, timeout, or engine failure.
pub fn analyze(
    path: &Path,
    fen: &str,
    nodes: u64,
    cancel: &Cancellation,
    timeout: Duration,
) -> Result<Analysis, Error> {
    let position = GamePosition::from_fen(fen)?;
    analyze_game(path, &position, nodes, cancel, timeout)
}

/// Analyze with the complete legal move history from the supplied starting FEN.
/// This preserves repetition evidence that a final-position FEN alone loses.
/// Setup games can preserve only the history actually supplied by their PGN.
///
/// # Errors
/// Returns an error on invalid budget, cancellation, timeout, or engine failure.
pub fn analyze_game(
    path: &Path,
    position: &GamePosition,
    nodes: u64,
    cancel: &Cancellation,
    timeout: Duration,
) -> Result<Analysis, Error> {
    if nodes == 0 {
        return Err(Error::InvalidPosition);
    }
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let deadline = Instant::now().checked_add(timeout).ok_or(Error::Timeout)?;
    let mut process = Process::spawn(path)?;
    process.send("uci")?;
    let mut identity = None;
    loop {
        let line = process.next(cancel, deadline)?;
        if line == "uciok" {
            break;
        }
        if let Some(name) = line.strip_prefix("id name ") {
            identity = Some(name.to_owned());
        }
    }
    let engine = identity
        .filter(|s| !s.is_empty())
        .ok_or(Error::Protocol("missing identity"))?;
    for command in [
        "setoption name Threads value 1",
        "setoption name Hash value 16",
        "setoption name MultiPV value 1",
        "ucinewgame",
        "isready",
    ] {
        process.send(command)?;
    }
    process.until("readyok", cancel, deadline)?;
    process.send(&position.command())?;
    process.send(&format!("go nodes {nodes}"))?;
    let mut evidence: VecDeque<Info> = VecDeque::new();
    loop {
        let line = process.next(cancel, deadline)?;
        if let Some(rest) = line.strip_prefix("bestmove ") {
            let best = rest
                .split_whitespace()
                .next()
                .ok_or(Error::Protocol("missing bestmove"))?;
            let best_move = if best == "(none)" || best == "0000" {
                None
            } else if uci_move(best) {
                Some(best.to_owned())
            } else {
                return Err(Error::Protocol("invalid bestmove"));
            };
            // A node limit can interrupt aspiration-window search. Match the
            // chosen move to its most recent evidence, preserving bound/depth.
            let latest = evidence
                .iter()
                .rev()
                .find(|info| best_move.as_ref() == info.pv.first())
                .ok_or(Error::Protocol("missing score for bestmove"))?;
            // Prefer the immediately previous exact iteration only when its
            // chosen root move is unchanged. Never search backwards for an old
            // score of a move that just replaced a different completed PV.
            let completed = evidence
                .iter()
                .rev()
                .find(|info| info.bound == ScoreBound::Exact);
            let (info, score_source) =
                match completed {
                    Some(info)
                        if latest.bound != ScoreBound::Exact
                            && info.pv.first() == best_move.as_ref()
                            && info.depth.zip(latest.depth).is_some_and(
                                |(previous, current)| previous.checked_add(1) == Some(current),
                            ) =>
                    {
                        (info, ScoreSource::PreviousCompletedIteration)
                    }
                    _ => (latest, ScoreSource::LatestReport),
                };
            let pv_san = legal_variation(position.position(), &info.pv)?;
            return Ok(Analysis {
                engine,
                nodes_budget: nodes,
                score: info.score,
                score_bound: info.bound,
                score_source,
                depth: info.depth,
                best_move,
                pv: info.pv.clone(),
                pv_san,
            });
        }
        if let Some(info) = parse_info(&line)? {
            if evidence.len() == MAX_EVIDENCE {
                evidence.pop_front();
            }
            evidence.push_back(info);
        }
    }
}

fn legal_variation(
    mut position: gambit_chess::Position,
    pv: &[String],
) -> Result<Vec<String>, Error> {
    let mut san = Vec::with_capacity(pv.len());
    let mut legal = gambit_chess::MoveList::default();
    if pv.is_empty() {
        position.generate_legal_moves(&mut legal);
        if !legal.is_empty() {
            return Err(Error::Protocol("missing move in nonterminal position"));
        }
    }
    for notation in pv {
        position.generate_legal_moves(&mut legal);
        let chess_move = legal
            .as_slice()
            .iter()
            .find(|m| m.to_uci() == *notation)
            .copied()
            .ok_or(Error::Protocol("illegal variation"))?;
        san.push(
            position
                .to_san(chess_move)
                .map_err(|_| Error::Protocol("illegal variation"))?,
        );
        position.play_unchecked(chess_move);
    }
    Ok(san)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_stockfish_transcript() {
        // Official arm64 17.1, startpos, Threads=1, Hash=16, 100,000 nodes.
        let transcript = include_str!("../tests/fixtures/stockfish-17.1-startpos.uci");
        let evidence: Vec<_> = transcript
            .lines()
            .filter_map(|line| parse_info(line).unwrap())
            .collect();
        assert_eq!(evidence.len(), 17);
        let info = evidence.last().unwrap();
        assert_eq!(info.score, Score::Centipawns(37));
        assert_eq!(info.bound, ScoreBound::Exact);
        assert_eq!(info.pv[0], "e2e4");
        assert_eq!(info.pv.len(), 18);
    }

    #[test]
    fn scores_and_bounds() {
        assert_eq!(
            parse_info("info depth 12 score cp -34 nodes 200 pv e7e5 g1f3").unwrap(),
            Some(Info {
                score: Score::Centipawns(-34),
                bound: ScoreBound::Exact,
                depth: Some(12),
                pv: vec!["e7e5".into(), "g1f3".into()]
            })
        );
        assert_eq!(
            parse_info("info score mate -3 pv e1e2")
                .unwrap()
                .unwrap()
                .score,
            Score::Mate(-3)
        );
        for line in ["info multipv 2 score cp 2", "info string score cp broken"] {
            assert_eq!(parse_info(line).unwrap(), None);
        }
        for line in [
            "info score cp",
            "info score cp -2147483648",
            "info score unknown 2",
            "info score cp 1 pv xyz",
            "info score cp 1 lowerbound upperbound",
        ] {
            assert!(parse_info(line).is_err());
        }
    }

    #[test]
    fn perspective_preserves_mate() {
        assert_eq!(
            Score::Centipawns(42).for_player(false),
            Score::Centipawns(-42)
        );
        assert_eq!(Score::Mate(-3).for_player(false), Score::Mate(3));
        assert_eq!(Score::Mate(0).for_player(true), Score::Mate(0));
        assert_eq!(ScoreBound::Lower.for_player(false), ScoreBound::Upper);
        assert_eq!(ScoreBound::Upper.for_player(false), ScoreBound::Lower);
        assert_eq!(ScoreBound::Exact.for_player(false), ScoreBound::Exact);
        assert_eq!(
            parse_info("info score cp 42 lowerbound")
                .unwrap()
                .unwrap()
                .bound,
            ScoreBound::Lower
        );
        assert_eq!(
            parse_info("info score mate -3 upperbound")
                .unwrap()
                .unwrap()
                .bound,
            ScoreBound::Upper
        );
    }

    #[test]
    fn bounded_input() {
        assert!(read_line(&mut &vec![b'x'; MAX_LINE + 1][..]).is_err());
        assert!(read_line(&mut &b"partial"[..]).is_err());
        assert!(read_line(&mut &b"\xff\n"[..]).is_err());
        assert_eq!(
            read_line(&mut &b"uciok\r\n"[..]).unwrap(),
            Some("uciok".into())
        );
        let line = format!("info score cp 0 pv {}", "e2e4 ".repeat(100));
        assert_eq!(parse_info(&line).unwrap().unwrap().pv.len(), MAX_PV);
    }
}
