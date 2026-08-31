use std::error::Error;
use std::fmt;

use crate::{Move, MoveList, Piece, Position, Square};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SanError {
    pub kind: SanErrorKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SanErrorKind {
    Empty,
    InvalidSyntax,
    IllegalMove,
    AmbiguousMove,
    InvalidCheckSuffix,
}

impl fmt::Display for SanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.kind {
            SanErrorKind::Empty => "empty SAN token",
            SanErrorKind::InvalidSyntax => "invalid SAN syntax",
            SanErrorKind::IllegalMove => "SAN does not identify a legal move",
            SanErrorKind::AmbiguousMove => "SAN identifies more than one legal move",
            SanErrorKind::InvalidCheckSuffix => "SAN check or mate suffix is incorrect",
        })
    }
}

impl Error for SanError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CheckSuffix {
    None,
    Check,
    Mate,
}

#[derive(Clone, Copy)]
struct ParsedSan {
    piece: Piece,
    destination: Square,
    from_file: Option<u8>,
    from_rank: Option<u8>,
    promotion: Option<Piece>,
    capture: bool,
    castle: bool,
    check: CheckSuffix,
}

impl Position {
    /// Resolves a SAN token against this position and applies the unique legal move.
    ///
    /// # Errors
    ///
    /// Returns [`SanError`] for malformed, illegal, ambiguous, or incorrectly
    /// check-annotated SAN.
    pub fn play_san(&mut self, san: &[u8]) -> Result<Move, SanError> {
        let parsed = parse_san(san)?;
        let chess_move = if parsed.castle {
            find_castle(*self, parsed)?
        } else {
            find_targeted_move(*self, parsed)?
        };

        let mut next = *self;
        next.play_unchecked(chess_move);
        validate_check_suffix(next, parsed.check)?;
        *self = next;
        Ok(chess_move)
    }
}

fn parse_san(mut san: &[u8]) -> Result<ParsedSan, SanError> {
    while matches!(san.last(), Some(b'!' | b'?')) {
        san = &san[..san.len() - 1];
    }
    if san.is_empty() {
        return Err(SanError {
            kind: SanErrorKind::Empty,
        });
    }
    let check = if san.ends_with(b"#") {
        san = &san[..san.len() - 1];
        CheckSuffix::Mate
    } else if san.ends_with(b"++") {
        san = &san[..san.len() - 2];
        CheckSuffix::Check
    } else if san.ends_with(b"+") {
        san = &san[..san.len() - 1];
        CheckSuffix::Check
    } else {
        CheckSuffix::None
    };

    if matches!(san, b"O-O" | b"0-0") {
        return Ok(ParsedSan {
            piece: Piece::King,
            destination: Square::G1,
            from_file: None,
            from_rank: None,
            promotion: None,
            capture: false,
            castle: true,
            check,
        });
    }
    if matches!(san, b"O-O-O" | b"0-0-0") {
        return Ok(ParsedSan {
            piece: Piece::King,
            destination: Square::C1,
            from_file: None,
            from_rank: None,
            promotion: None,
            capture: false,
            castle: true,
            check,
        });
    }

    let (without_promotion, promotion) = parse_promotion(san)?;
    san = without_promotion;
    if san.len() < 2 {
        return Err(invalid_syntax());
    }
    let destination = Square::from_ascii(&san[san.len() - 2..]).ok_or_else(invalid_syntax)?;
    let mut prefix = &san[..san.len() - 2];
    let piece = match prefix.first() {
        Some(b'K') => Piece::King,
        Some(b'Q') => Piece::Queen,
        Some(b'R') => Piece::Rook,
        Some(b'B') => Piece::Bishop,
        Some(b'N') => Piece::Knight,
        _ => Piece::Pawn,
    };
    if piece != Piece::Pawn {
        prefix = &prefix[1..];
    }
    let capture = if prefix.ends_with(b"x") {
        prefix = &prefix[..prefix.len() - 1];
        true
    } else {
        false
    };
    if prefix.contains(&b'x') || prefix.len() > 2 {
        return Err(invalid_syntax());
    }

    let mut from_file = None;
    let mut from_rank = None;
    for byte in prefix {
        match byte {
            b'a'..=b'h' if from_file.is_none() => from_file = Some(*byte - b'a'),
            b'1'..=b'8' if from_rank.is_none() => from_rank = Some(*byte - b'1'),
            _ => return Err(invalid_syntax()),
        }
    }
    if piece == Piece::Pawn && (capture != from_file.is_some() || from_rank.is_some()) {
        return Err(invalid_syntax());
    }
    if promotion.is_some() != (piece == Piece::Pawn && matches!(destination.rank(), 0 | 7)) {
        return Err(invalid_syntax());
    }

    Ok(ParsedSan {
        piece,
        destination,
        from_file,
        from_rank,
        promotion,
        capture,
        castle: false,
        check,
    })
}

fn parse_promotion(san: &[u8]) -> Result<(&[u8], Option<Piece>), SanError> {
    if san.len() >= 2 && san[san.len() - 2] == b'=' {
        let piece = match san[san.len() - 1] {
            b'Q' => Piece::Queen,
            b'R' => Piece::Rook,
            b'B' => Piece::Bishop,
            b'N' => Piece::Knight,
            _ => return Err(invalid_syntax()),
        };
        Ok((&san[..san.len() - 2], Some(piece)))
    } else {
        Ok((san, None))
    }
}

fn find_castle(position: Position, parsed: ParsedSan) -> Result<Move, SanError> {
    let mut legal = MoveList::default();
    position.generate_legal_moves(&mut legal);
    let destination = if parsed.castle {
        match (position.side_to_move(), parsed.destination) {
            (crate::Color::Black, Square::G1) => Square::G8,
            (crate::Color::Black, Square::C1) => Square::C8,
            (_, destination) => destination,
        }
    } else {
        parsed.destination
    };
    legal
        .as_slice()
        .iter()
        .copied()
        .find(|chess_move| chess_move.is_castle() && chess_move.to() == destination)
        .ok_or(SanError {
            kind: SanErrorKind::IllegalMove,
        })
}

fn find_targeted_move(position: Position, parsed: ParsedSan) -> Result<Move, SanError> {
    let color = position.side_to_move();
    let mut sources = position.bitboard(color, parsed.piece);
    let mut matched = None;
    while sources != 0 {
        let from = Square::from_index(
            u8::try_from(sources.trailing_zeros()).expect("nonzero bit index is below 64"),
        )
        .expect("bitboard square is valid");
        sources &= sources - 1;
        if parsed.from_file.is_some_and(|file| from.file() != file)
            || parsed.from_rank.is_some_and(|rank| from.rank() != rank)
        {
            continue;
        }
        let Some(chess_move) = candidate_move(position, from, parsed) else {
            continue;
        };
        let mut next = position;
        next.play_unchecked(chess_move);
        if next.in_check(color) {
            continue;
        }
        if matched.is_some() {
            return Err(SanError {
                kind: SanErrorKind::AmbiguousMove,
            });
        }
        matched = Some(chess_move);
    }
    matched.ok_or(SanError {
        kind: SanErrorKind::IllegalMove,
    })
}

fn candidate_move(position: Position, from: Square, parsed: ParsedSan) -> Option<Move> {
    let color = position.side_to_move();
    let destination = parsed.destination;
    let destination_piece = position.piece_at(destination);
    if destination_piece.is_some_and(|(piece_color, _)| piece_color == color) {
        return None;
    }
    let actual_capture = destination_piece.is_some();
    let mut flags = 0;

    let reaches = match parsed.piece {
        Piece::Pawn => {
            let direction = if color == crate::Color::White { 1 } else { -1 };
            let file_delta = i16::from(destination.file()) - i16::from(from.file());
            let rank_delta = i16::from(destination.rank()) - i16::from(from.rank());
            if parsed.capture {
                let en_passant = position.en_passant() == Some(destination) && !actual_capture;
                if en_passant {
                    flags |= Move::EN_PASSANT;
                }
                file_delta.abs() == 1 && rank_delta == direction && (actual_capture || en_passant)
            } else if file_delta == 0 && !actual_capture {
                if rank_delta == direction {
                    true
                } else {
                    let start_rank = if color == crate::Color::White { 1 } else { 6 };
                    let middle_index = i16::from(from.index()) + direction * 8;
                    let middle = Square::from_index(u8::try_from(middle_index).ok()?)?;
                    let double = from.rank() == start_rank
                        && rank_delta == direction * 2
                        && position.piece_at(middle).is_none();
                    if double {
                        flags |= Move::DOUBLE_PAWN;
                    }
                    double
                }
            } else {
                false
            }
        }
        Piece::Knight => {
            let file = file_distance(from, destination);
            let rank = rank_distance(from, destination);
            matches!((file, rank), (1, 2) | (2, 1))
        }
        Piece::Bishop => {
            file_distance(from, destination) == rank_distance(from, destination)
                && path_is_clear(position, from, destination)
        }
        Piece::Rook => {
            (from.file() == destination.file() || from.rank() == destination.rank())
                && path_is_clear(position, from, destination)
        }
        Piece::Queen => {
            (from.file() == destination.file()
                || from.rank() == destination.rank()
                || file_distance(from, destination) == rank_distance(from, destination))
                && path_is_clear(position, from, destination)
        }
        Piece::King => {
            file_distance(from, destination) <= 1 && rank_distance(from, destination) <= 1
        }
    };
    if !reaches || parsed.capture != (actual_capture || flags & Move::EN_PASSANT != 0) {
        return None;
    }
    if parsed.capture {
        flags |= Move::CAPTURE;
    }
    Some(Move::new(from, destination, parsed.promotion, flags))
}

fn path_is_clear(position: Position, from: Square, to: Square) -> bool {
    let file_step = sign(i16::from(to.file()) - i16::from(from.file()));
    let rank_step = sign(i16::from(to.rank()) - i16::from(from.rank()));
    if file_step == 0 && rank_step == 0 {
        return false;
    }
    let mut file = i16::from(from.file()) + file_step;
    let mut rank = i16::from(from.rank()) + rank_step;
    while file != i16::from(to.file()) || rank != i16::from(to.rank()) {
        let square = Square::from_coords(
            u8::try_from(file).ok().unwrap(),
            u8::try_from(rank).ok().unwrap(),
        )
        .expect("path remains on board");
        if position.piece_at(square).is_some() {
            return false;
        }
        file += file_step;
        rank += rank_step;
    }
    true
}

fn sign(value: i16) -> i16 {
    value.signum()
}

fn file_distance(from: Square, to: Square) -> u8 {
    from.file().abs_diff(to.file())
}

fn rank_distance(from: Square, to: Square) -> u8 {
    from.rank().abs_diff(to.rank())
}

fn validate_check_suffix(position: Position, suffix: CheckSuffix) -> Result<(), SanError> {
    match suffix {
        CheckSuffix::None => Ok(()),
        CheckSuffix::Check if position.in_check(position.side_to_move()) => Ok(()),
        CheckSuffix::Mate if position.in_check(position.side_to_move()) => {
            let mut replies = MoveList::default();
            position.generate_legal_moves(&mut replies);
            if replies.is_empty() {
                Ok(())
            } else {
                Err(SanError {
                    kind: SanErrorKind::InvalidCheckSuffix,
                })
            }
        }
        CheckSuffix::Check | CheckSuffix::Mate => Err(SanError {
            kind: SanErrorKind::InvalidCheckSuffix,
        }),
    }
}

fn invalid_syntax() -> SanError {
    SanError {
        kind: SanErrorKind::InvalidSyntax,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Piece};

    fn play_line(line: &[&[u8]]) -> Position {
        let mut position = Position::initial();
        for san in line {
            position.play_san(san).unwrap_or_else(|error| {
                panic!("{}: {error}", String::from_utf8_lossy(san));
            });
        }
        position
    }

    #[test]
    fn plays_opening_castles_and_tracks_state() {
        let position = play_line(&[
            b"e4", b"e5", b"Nf3", b"Nc6", b"Bb5", b"a6", b"Ba4", b"Nf6", b"O-O",
        ]);
        assert_eq!(position.side_to_move(), Color::Black);
        assert_eq!(
            position.piece_at(Square::G1),
            Some((Color::White, Piece::King))
        );
        assert_eq!(
            position.piece_at(Square::F1),
            Some((Color::White, Piece::Rook))
        );
    }

    #[test]
    fn rejects_ambiguous_san_and_accepts_disambiguation() {
        let position = Position::from_fen(b"4k3/8/8/8/8/2N1N3/8/4K3 w - - 0 1").unwrap();
        let mut ambiguous = position;
        assert_eq!(
            ambiguous.play_san(b"Nd5").unwrap_err().kind,
            SanErrorKind::AmbiguousMove
        );
        let mut resolved = position;
        assert!(resolved.play_san(b"Ncd5").is_ok());
    }

    #[test]
    fn handles_en_passant_and_promotion_mate() {
        let mut en_passant = Position::from_fen(b"4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
        let chess_move = en_passant.play_san(b"exd6").unwrap();
        assert!(chess_move.is_en_passant());

        let mut promotion = Position::from_fen(b"7k/5KP1/8/8/8/8/8/8 w - - 0 1").unwrap();
        assert!(promotion.play_san(b"g8=Q#").is_ok());
    }

    #[test]
    fn recognizes_fools_mate() {
        let position = play_line(&[b"f3", b"e5", b"g4", b"Qh4#"]);
        assert!(position.in_check(Color::White));
        let mut replies = MoveList::default();
        position.generate_legal_moves(&mut replies);
        assert!(replies.is_empty());
    }
}
