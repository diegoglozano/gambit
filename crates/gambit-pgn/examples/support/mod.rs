use std::fmt;

use gambit_chess::Position;
use gambit_pgn::{Event, Parser};

#[derive(Debug)]
pub(crate) struct SemanticError {
    pub(crate) game: u64,
    pub(crate) ply: u64,
    pub(crate) offset: u64,
    pub(crate) kind: &'static str,
    pub(crate) context: Vec<u8>,
    pub(crate) detail: String,
}

#[derive(Debug)]
#[allow(dead_code)] // This shared module is compiled separately by each example.
pub(crate) enum GameValidationError {
    Parse {
        game: u64,
        offset: u64,
        detail: String,
    },
    Semantic(SemanticError),
}

impl fmt::Display for GameValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse {
                game,
                offset,
                detail,
            } => write!(f, "game {game}, byte {offset}: {detail}"),
            Self::Semantic(error) => write!(
                f,
                "game {}, ply {}, byte {}: {} {}: {}",
                error.game,
                error.ply,
                error.offset,
                error.kind,
                String::from_utf8_lossy(&error.context),
                error.detail
            ),
        }
    }
}

#[allow(dead_code)] // The incremental example consumes events directly.
pub(crate) fn validate_game(
    input: &[u8],
    game_number: u64,
    offset: u64,
) -> Result<u64, GameValidationError> {
    let mut validator = Validator::with_origin(game_number - 1, offset);
    for event in Parser::new(input) {
        match event {
            Ok(event) => validator.observe(event),
            Err(error) => {
                return Err(GameValidationError::Parse {
                    game: game_number,
                    offset: offset
                        + u64::try_from(error.offset).expect("parse error offset fits in u64"),
                    detail: error.to_string(),
                });
            }
        }
        if let Some(error) = validator.error.take() {
            return Err(GameValidationError::Semantic(error));
        }
    }
    debug_assert_eq!(validator.games, game_number);
    Ok(validator.moves)
}

pub(crate) struct Validator {
    position: Position,
    before_last_move: Position,
    variation_stack: Vec<(Position, Position)>,
    fen: Option<(Vec<u8>, usize)>,
    pub(crate) games: u64,
    pub(crate) moves: u64,
    game_ply: u64,
    offset_base: u64,
    pub(crate) error: Option<SemanticError>,
}

impl Default for Validator {
    fn default() -> Self {
        Self::with_origin(0, 0)
    }
}

impl Validator {
    /// Creates a validator whose next game has number `completed_games + 1`.
    pub(crate) fn with_origin(completed_games: u64, offset_base: u64) -> Self {
        Self {
            position: Position::initial(),
            before_last_move: Position::initial(),
            variation_stack: Vec::new(),
            fen: None,
            games: completed_games,
            moves: 0,
            game_ply: 0,
            offset_base,
            error: None,
        }
    }

    pub(crate) fn observe(&mut self, event: Event<'_>) {
        if self.error.is_some() {
            return;
        }
        match event {
            Event::GameStart { .. } => {
                self.position = Position::initial();
                self.before_last_move = self.position;
                self.variation_stack.clear();
                self.fen = None;
                self.game_ply = 0;
            }
            Event::Tag(tag) if tag.name() == b"FEN" => {
                self.fen = Some((tag.raw_value().to_vec(), tag.span().start));
            }
            Event::MovetextStart { .. } => {
                if let Some((fen, offset)) = &self.fen {
                    match Position::from_fen(fen) {
                        Ok(position) => self.position = position,
                        Err(error) => {
                            self.error = Some(SemanticError {
                                game: self.games + 1,
                                ply: 0,
                                offset: self.offset_base
                                    + u64::try_from(*offset).expect("span offset fits in u64"),
                                kind: "FEN",
                                context: fen.clone(),
                                detail: error.to_string(),
                            });
                            return;
                        }
                    }
                }
                self.before_last_move = self.position;
            }
            Event::San(token) => {
                self.before_last_move = self.position;
                self.game_ply += 1;
                match self.position.play_san(token.as_bytes()) {
                    Ok(_) => self.moves += 1,
                    Err(source) => {
                        self.error = Some(SemanticError {
                            game: self.games + 1,
                            ply: self.game_ply,
                            offset: self.offset_base
                                + u64::try_from(token.span().start)
                                    .expect("span offset fits in u64"),
                            kind: "SAN",
                            context: token.as_bytes().to_vec(),
                            detail: source.to_string(),
                        });
                    }
                }
            }
            Event::VariationStart(_) => {
                self.variation_stack
                    .push((self.position, self.before_last_move));
                let branch_base = self.before_last_move;
                self.position = branch_base;
                self.before_last_move = branch_base;
            }
            Event::VariationEnd(_) => {
                if let Some((position, before_last_move)) = self.variation_stack.pop() {
                    self.position = position;
                    self.before_last_move = before_last_move;
                }
            }
            Event::GameEnd { .. } => self.games += 1,
            Event::Tag(_)
            | Event::MoveNumber { .. }
            | Event::Nag(_)
            | Event::Comment(_)
            | Event::Outcome { .. } => {}
        }
    }
}
