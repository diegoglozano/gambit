use gambit_chess::Color;
use gambit_engine::GamePosition;
use gambit_pgn::{Event, Parser, ParserOptions};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputError {
    TooLarge,
    ExpectedOneGame,
    PlayerRequired,
    PlayerMissingOrAmbiguous,
    UnsupportedVariant,
    InvalidPgn(String),
    InvalidSetup,
    IllegalMainline { ply: usize },
}

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for InputError {}

#[derive(Debug, Clone)]
pub struct ReviewGame {
    initial: GamePosition,
    player: Color,
    player_name: String,
    mainline: Vec<(String, String)>,
}

#[derive(Debug)]
pub struct Decision {
    /// One-based mainline ply, not a move number from a setup FEN.
    pub ply: usize,
    pub before: GamePosition,
    pub after: GamePosition,
    pub played_uci: String,
    pub played_san: String,
}

impl ReviewGame {
    pub const MAX_PGN_BYTES: usize = 256 * 1024;

    /// Replay exactly one standard game and attribute moves to an explicit player.
    /// Variations/comments do not become history. Empty and short games are valid.
    ///
    /// # Errors
    /// Rejects oversized, malformed, nonstandard, ambiguous, or illegal evidence.
    #[allow(clippy::too_many_lines)]
    pub fn parse(pgn: &[u8], player: &str) -> Result<Self, InputError> {
        if pgn.len() > Self::MAX_PGN_BYTES {
            return Err(InputError::TooLarge);
        }
        let player = player.trim();
        if player.is_empty() || player.len() > 256 {
            return Err(InputError::PlayerRequired);
        }
        let (mut white, mut black, mut fen, mut setup, mut variant) =
            (None, None, None, None, None);
        let mut initial = GamePosition::default();
        let mut history = initial.clone();
        let mut selected = None;
        let mut mainline = Vec::new();
        let mut starts = 0;
        let mut ends = 0;
        let mut depth = 0;
        for event in Parser::with_options(pgn, ParserOptions::STRICT) {
            match event.map_err(|e| InputError::InvalidPgn(e.to_string()))? {
                Event::GameStart { .. } => {
                    starts += 1;
                    if starts > 1 {
                        return Err(InputError::ExpectedOneGame);
                    }
                }
                Event::GameEnd { .. } => ends += 1,
                Event::Tag(tag) => {
                    let target = match tag.name() {
                        b"White" => &mut white,
                        b"Black" => &mut black,
                        b"FEN" => &mut fen,
                        b"SetUp" => &mut setup,
                        b"Variant" => &mut variant,
                        _ => continue,
                    };
                    if target.is_some() {
                        return Err(InputError::InvalidPgn(
                            "duplicate identity/setup tag".into(),
                        ));
                    }
                    *target = Some(
                        String::from_utf8(tag.value().into_owned())
                            .map_err(|_| InputError::InvalidPgn("invalid UTF-8 tag".into()))?,
                    );
                }
                Event::MovetextStart { .. } => {
                    if variant
                        .as_deref()
                        .is_some_and(|v| !v.trim().eq_ignore_ascii_case("standard"))
                    {
                        return Err(InputError::UnsupportedVariant);
                    }
                    let matches = |name: &Option<String>| {
                        name.as_deref()
                            .is_some_and(|n| n.trim().eq_ignore_ascii_case(player))
                    };
                    selected = Some(match (matches(&white), matches(&black)) {
                        (true, false) => Color::White,
                        (false, true) => Color::Black,
                        _ => return Err(InputError::PlayerMissingOrAmbiguous),
                    });
                    match setup.as_deref() {
                        Some("1") if fen.is_none() => return Err(InputError::InvalidSetup),
                        Some("0") if fen.is_some() => return Err(InputError::InvalidSetup),
                        Some("0" | "1") | None => {}
                        _ => return Err(InputError::InvalidSetup),
                    }
                    if let Some(fen) = &fen {
                        initial =
                            GamePosition::from_fen(fen).map_err(|_| InputError::InvalidSetup)?;
                        history = initial.clone();
                    }
                }
                Event::VariationStart(_) => depth += 1,
                Event::VariationEnd(_) => depth -= 1,
                Event::San(token) if depth == 0 => {
                    if mainline.len() >= GamePosition::MAX_PLIES {
                        return Err(InputError::TooLarge);
                    }
                    let ply = mainline.len() + 1;
                    let before = history.position();
                    let mut next = before;
                    let chess_move = next
                        .play_san(token.as_bytes())
                        .map_err(|_| InputError::IllegalMainline { ply })?;
                    let san = before
                        .to_san(chess_move)
                        .map_err(|_| InputError::IllegalMainline { ply })?;
                    let uci = chess_move.to_uci();
                    history
                        .play_uci(&uci)
                        .map_err(|_| InputError::IllegalMainline { ply })?;
                    mainline.push((uci, san));
                }
                _ => {}
            }
        }
        if starts != 1 || ends != 1 {
            return Err(InputError::ExpectedOneGame);
        }
        Ok(Self {
            initial,
            player: selected.ok_or(InputError::PlayerMissingOrAmbiguous)?,
            player_name: player.to_ascii_lowercase(),
            mainline,
        })
    }

    #[must_use]
    pub fn player(&self) -> Color {
        self.player
    }

    #[must_use]
    pub fn player_name(&self) -> &str {
        &self.player_name
    }

    #[must_use]
    pub fn initial_fen(&self) -> &str {
        self.initial.initial_fen()
    }

    #[must_use]
    pub fn mainline(&self) -> &[(String, String)] {
        &self.mainline
    }

    /// Select only this player's moves whose pre-move ply is at least `shared_ply`.
    /// Histories are constructed lazily so full before/after histories are not
    /// retained for every move in a long game at the same time.
    pub fn decisions(
        &self,
        shared_ply: usize,
    ) -> impl Iterator<Item = Result<Decision, InputError>> + '_ {
        let mut history = self.initial.clone();
        self.mainline
            .iter()
            .enumerate()
            .filter_map(move |(index, (uci, san))| {
                let include =
                    index >= shared_ply && history.position().side_to_move() == self.player;
                let before = include.then(|| history.clone());
                if history.play_uci(uci).is_err() {
                    return Some(Err(InputError::IllegalMainline { ply: index + 1 }));
                }
                before.map(|before| {
                    Ok(Decision {
                        ply: index + 1,
                        before,
                        after: history.clone(),
                        played_uci: uci.clone(),
                        played_san: san.clone(),
                    })
                })
            })
    }
}
