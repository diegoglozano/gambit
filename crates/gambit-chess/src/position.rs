use std::error::Error;
use std::fmt;

const NO_SQUARE: u8 = 64;
const MAX_LEGAL_MOVES: usize = 256;
const KNIGHT_ATTACKS: [u64; 64] = build_attack_table([
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
]);
const KING_ATTACKS: [u64; 64] = build_attack_table([
    (1, 1),
    (1, 0),
    (1, -1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
]);
const WHITE_PAWN_ATTACKERS: [u64; 64] = build_attack_table([(-1, -1), (1, -1)]);
const BLACK_PAWN_ATTACKERS: [u64; 64] = build_attack_table([(-1, 1), (1, 1)]);
const NORTH_EAST_RAYS: [u64; 64] = build_ray_table(1, 1);
const NORTH_WEST_RAYS: [u64; 64] = build_ray_table(-1, 1);
const SOUTH_EAST_RAYS: [u64; 64] = build_ray_table(1, -1);
const SOUTH_WEST_RAYS: [u64; 64] = build_ray_table(-1, -1);
const EAST_RAYS: [u64; 64] = build_ray_table(1, 0);
const NORTH_RAYS: [u64; 64] = build_ray_table(0, 1);
const WEST_RAYS: [u64; 64] = build_ray_table(-1, 0);
const SOUTH_RAYS: [u64; 64] = build_ray_table(0, -1);

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]
const fn build_attack_table<const N: usize>(deltas: [(i8, i8); N]) -> [u64; 64] {
    let mut table = [0_u64; 64];
    let mut index = 0_usize;
    while index < table.len() {
        let file = (index & 7) as i8;
        let rank = (index >> 3) as i8;
        let mut delta = 0_usize;
        while delta < deltas.len() {
            let target_file = file + deltas[delta].0;
            let target_rank = rank + deltas[delta].1;
            if target_file >= 0 && target_file < 8 && target_rank >= 0 && target_rank < 8 {
                let target = (target_rank as usize) * 8 + target_file as usize;
                table[index] |= 1_u64 << target;
            }
            delta += 1;
        }
        index += 1;
    }
    table
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]
const fn build_ray_table(file_delta: i8, rank_delta: i8) -> [u64; 64] {
    let mut table = [0_u64; 64];
    let mut index = 0_usize;
    while index < table.len() {
        let mut file = (index & 7) as i8 + file_delta;
        let mut rank = (index >> 3) as i8 + rank_delta;
        while file >= 0 && file < 8 && rank >= 0 && rank < 8 {
            let target = (rank as usize) * 8 + file as usize;
            table[index] |= 1_u64 << target;
            file += file_delta;
            rank += rank_delta;
        }
        index += 1;
    }
    table
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Color {
    White,
    Black,
}

impl Color {
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::White => Self::Black,
            Self::Black => Self::White,
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Piece {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

impl Piece {
    const ALL: [Self; 6] = [
        Self::Pawn,
        Self::Knight,
        Self::Bishop,
        Self::Rook,
        Self::Queen,
        Self::King,
    ];

    const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Square(u8);

impl Square {
    pub const A1: Self = Self(0);
    pub const C1: Self = Self(2);
    pub const D1: Self = Self(3);
    pub const E1: Self = Self(4);
    pub const F1: Self = Self(5);
    pub const G1: Self = Self(6);
    pub const H1: Self = Self(7);
    pub const A8: Self = Self(56);
    pub const C8: Self = Self(58);
    pub const D8: Self = Self(59);
    pub const E8: Self = Self(60);
    pub const F8: Self = Self(61);
    pub const G8: Self = Self(62);
    pub const H8: Self = Self(63);

    #[must_use]
    pub const fn from_index(index: u8) -> Option<Self> {
        if index < 64 { Some(Self(index)) } else { None }
    }

    #[must_use]
    pub const fn from_coords(file: u8, rank: u8) -> Option<Self> {
        if file < 8 && rank < 8 {
            Some(Self(rank * 8 + file))
        } else {
            None
        }
    }

    #[must_use]
    pub fn from_ascii(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 2 {
            return None;
        }
        Self::from_coords(bytes[0].checked_sub(b'a')?, bytes[1].checked_sub(b'1')?)
    }

    #[must_use]
    pub const fn index(self) -> u8 {
        self.0
    }

    #[must_use]
    pub const fn file(self) -> u8 {
        self.0 & 7
    }

    #[must_use]
    pub const fn rank(self) -> u8 {
        self.0 >> 3
    }

    const fn bit(self) -> u64 {
        1_u64 << self.0
    }
}

impl fmt::Display for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf8_lossy(&[
            b'a' + self.file(),
            b'1' + self.rank(),
        ]))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CastlingRights(u8);

impl CastlingRights {
    pub const WHITE_KINGSIDE: u8 = 1;
    pub const WHITE_QUEENSIDE: u8 = 2;
    pub const BLACK_KINGSIDE: u8 = 4;
    pub const BLACK_QUEENSIDE: u8 = 8;

    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    #[must_use]
    pub const fn contains(self, rights: u8) -> bool {
        self.0 & rights == rights
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Move(u32);

impl Move {
    pub(crate) const CAPTURE: u32 = 1 << 15;
    pub(crate) const EN_PASSANT: u32 = 1 << 16;
    pub(crate) const CASTLE: u32 = 1 << 17;
    pub(crate) const DOUBLE_PAWN: u32 = 1 << 18;

    pub(crate) const fn new(
        from: Square,
        to: Square,
        piece: Piece,
        promotion: Option<Piece>,
        flags: u32,
    ) -> Self {
        let promotion = match promotion {
            None | Some(Piece::Pawn | Piece::King) => 0,
            Some(Piece::Knight) => 1,
            Some(Piece::Bishop) => 2,
            Some(Piece::Rook) => 3,
            Some(Piece::Queen) => 4,
        };
        Self(
            (from.0 as u32)
                | ((to.0 as u32) << 6)
                | (promotion << 12)
                | flags
                | (((piece as u32) + 1) << 19),
        )
    }

    #[must_use]
    pub const fn from(self) -> Square {
        Square((self.0 & 0x3f) as u8)
    }

    #[must_use]
    pub const fn to(self) -> Square {
        Square(((self.0 >> 6) & 0x3f) as u8)
    }

    #[must_use]
    pub const fn promotion(self) -> Option<Piece> {
        match (self.0 >> 12) & 7 {
            1 => Some(Piece::Knight),
            2 => Some(Piece::Bishop),
            3 => Some(Piece::Rook),
            4 => Some(Piece::Queen),
            _ => None,
        }
    }

    const fn piece(self) -> Option<Piece> {
        match (self.0 >> 19) & 7 {
            1 => Some(Piece::Pawn),
            2 => Some(Piece::Knight),
            3 => Some(Piece::Bishop),
            4 => Some(Piece::Rook),
            5 => Some(Piece::Queen),
            6 => Some(Piece::King),
            _ => None,
        }
    }

    #[must_use]
    pub const fn is_capture(self) -> bool {
        self.0 & Self::CAPTURE != 0
    }

    #[must_use]
    pub const fn is_en_passant(self) -> bool {
        self.0 & Self::EN_PASSANT != 0
    }

    #[must_use]
    pub const fn is_castle(self) -> bool {
        self.0 & Self::CASTLE != 0
    }
}

#[derive(Clone, Debug)]
pub struct MoveList {
    moves: [Move; MAX_LEGAL_MOVES],
    len: u16,
}

impl Default for MoveList {
    fn default() -> Self {
        Self {
            moves: [Move::default(); MAX_LEGAL_MOVES],
            len: 0,
        }
    }
}

impl MoveList {
    #[must_use]
    pub fn as_slice(&self) -> &[Move] {
        &self.moves[..usize::from(self.len)]
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn clear(&mut self) {
        self.len = 0;
    }

    fn push(&mut self, chess_move: Move) {
        let index = usize::from(self.len);
        assert!(index < MAX_LEGAL_MOVES, "legal move list capacity exceeded");
        self.moves[index] = chess_move;
        self.len += 1;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Position {
    pieces: [u64; 12],
    side_to_move: Color,
    castling: CastlingRights,
    en_passant: u8,
    halfmove_clock: u16,
    fullmove_number: u16,
}

impl Default for Position {
    fn default() -> Self {
        Self::initial()
    }
}

impl Position {
    #[must_use]
    pub const fn initial() -> Self {
        Self {
            pieces: [
                0x0000_0000_0000_ff00,
                0x0000_0000_0000_0042,
                0x0000_0000_0000_0024,
                0x0000_0000_0000_0081,
                0x0000_0000_0000_0008,
                0x0000_0000_0000_0010,
                0x00ff_0000_0000_0000,
                0x4200_0000_0000_0000,
                0x2400_0000_0000_0000,
                0x8100_0000_0000_0000,
                0x0800_0000_0000_0000,
                0x1000_0000_0000_0000,
            ],
            side_to_move: Color::White,
            castling: CastlingRights(0x0f),
            en_passant: NO_SQUARE,
            halfmove_clock: 0,
            fullmove_number: 1,
        }
    }

    /// Parses all six fields of a Forsyth-Edwards Notation position.
    ///
    /// # Errors
    ///
    /// Returns [`FenError`] when the board or state fields are malformed.
    pub fn from_fen(fen: &[u8]) -> Result<Self, FenError> {
        let mut fields = fen.split(u8::is_ascii_whitespace);
        let board = next_nonempty(&mut fields).ok_or(FenError::MissingField)?;
        let side = next_nonempty(&mut fields).ok_or(FenError::MissingField)?;
        let castling = next_nonempty(&mut fields).ok_or(FenError::MissingField)?;
        let en_passant = next_nonempty(&mut fields).ok_or(FenError::MissingField)?;
        let halfmove = next_nonempty(&mut fields).ok_or(FenError::MissingField)?;
        let fullmove = next_nonempty(&mut fields).ok_or(FenError::MissingField)?;
        if next_nonempty(&mut fields).is_some() {
            return Err(FenError::TooManyFields);
        }

        let mut pieces = [0_u64; 12];
        let mut rank = 7_u8;
        let mut file = 0_u8;
        for byte in board {
            match *byte {
                b'/' if file == 8 && rank > 0 => {
                    rank -= 1;
                    file = 0;
                }
                b'1'..=b'8' => {
                    file = file
                        .checked_add(*byte - b'0')
                        .filter(|value| *value <= 8)
                        .ok_or(FenError::InvalidBoard)?;
                }
                piece_byte => {
                    if file >= 8 {
                        return Err(FenError::InvalidBoard);
                    }
                    let (color, piece) =
                        piece_from_fen(piece_byte).ok_or(FenError::InvalidBoard)?;
                    let square = Square::from_coords(file, rank).ok_or(FenError::InvalidBoard)?;
                    pieces[piece_index(color, piece)] |= square.bit();
                    file += 1;
                }
            }
        }
        if rank != 0 || file != 8 {
            return Err(FenError::InvalidBoard);
        }

        let side_to_move = match side {
            b"w" => Color::White,
            b"b" => Color::Black,
            _ => return Err(FenError::InvalidSideToMove),
        };
        let mut rights = 0_u8;
        if castling != b"-" {
            for byte in castling {
                rights |= match byte {
                    b'K' => CastlingRights::WHITE_KINGSIDE,
                    b'Q' => CastlingRights::WHITE_QUEENSIDE,
                    b'k' => CastlingRights::BLACK_KINGSIDE,
                    b'q' => CastlingRights::BLACK_QUEENSIDE,
                    _ => return Err(FenError::InvalidCastlingRights),
                };
            }
        }
        let en_passant = if en_passant == b"-" {
            NO_SQUARE
        } else {
            let square = Square::from_ascii(en_passant).ok_or(FenError::InvalidEnPassant)?;
            if !matches!(square.rank(), 2 | 5) {
                return Err(FenError::InvalidEnPassant);
            }
            square.0
        };
        let halfmove_clock = parse_u16(halfmove).ok_or(FenError::InvalidHalfmoveClock)?;
        let fullmove_number = parse_u16(fullmove)
            .filter(|number| *number != 0)
            .ok_or(FenError::InvalidFullmoveNumber)?;

        let position = Self {
            pieces,
            side_to_move,
            castling: CastlingRights(rights),
            en_passant,
            halfmove_clock,
            fullmove_number,
        };
        if position.bitboard(Color::White, Piece::King).count_ones() != 1
            || position.bitboard(Color::Black, Piece::King).count_ones() != 1
        {
            return Err(FenError::InvalidKings);
        }
        Ok(position)
    }

    #[must_use]
    pub const fn side_to_move(self) -> Color {
        self.side_to_move
    }

    #[must_use]
    pub const fn castling_rights(self) -> CastlingRights {
        self.castling
    }

    #[must_use]
    pub const fn en_passant(self) -> Option<Square> {
        Square::from_index(self.en_passant)
    }

    #[must_use]
    pub const fn halfmove_clock(self) -> u16 {
        self.halfmove_clock
    }

    #[must_use]
    pub const fn fullmove_number(self) -> u16 {
        self.fullmove_number
    }

    #[must_use]
    pub const fn bitboard(self, color: Color, piece: Piece) -> u64 {
        self.pieces[piece_index(color, piece)]
    }

    #[must_use]
    pub fn piece_at(self, square: Square) -> Option<(Color, Piece)> {
        for color in [Color::White, Color::Black] {
            for piece in Piece::ALL {
                if self.bitboard(color, piece) & square.bit() != 0 {
                    return Some((color, piece));
                }
            }
        }
        None
    }

    #[must_use]
    pub fn occupied_by(self, color: Color) -> u64 {
        Piece::ALL
            .into_iter()
            .fold(0, |occupied, piece| occupied | self.bitboard(color, piece))
    }

    #[must_use]
    pub fn occupied(self) -> u64 {
        self.occupied_by(Color::White) | self.occupied_by(Color::Black)
    }

    #[must_use]
    pub fn in_check(self, color: Color) -> bool {
        let king = self.bitboard(color, Piece::King);
        king != 0 && self.is_square_attacked(square_from_bit(king), color.opposite())
    }

    #[must_use]
    pub fn is_square_attacked(self, square: Square, by: Color) -> bool {
        let pawns = self.bitboard(by, Piece::Pawn);
        let square_index = usize::from(square.0);
        let pawn_attackers = pawn_attackers(square, by);
        if pawns & pawn_attackers != 0
            || self.bitboard(by, Piece::Knight) & knight_attacks(square) != 0
            || self.bitboard(by, Piece::King) & king_attacks(square) != 0
        {
            return true;
        }

        let occupied = self.occupied();
        let diagonal_hits = first_occupied_increasing(NORTH_EAST_RAYS[square_index], occupied)
            | first_occupied_increasing(NORTH_WEST_RAYS[square_index], occupied)
            | first_occupied_decreasing(SOUTH_EAST_RAYS[square_index], occupied)
            | first_occupied_decreasing(SOUTH_WEST_RAYS[square_index], occupied);
        if diagonal_hits & (self.bitboard(by, Piece::Bishop) | self.bitboard(by, Piece::Queen)) != 0
        {
            return true;
        }

        let orthogonal_hits = first_occupied_increasing(EAST_RAYS[square_index], occupied)
            | first_occupied_increasing(NORTH_RAYS[square_index], occupied)
            | first_occupied_decreasing(WEST_RAYS[square_index], occupied)
            | first_occupied_decreasing(SOUTH_RAYS[square_index], occupied);
        orthogonal_hits & (self.bitboard(by, Piece::Rook) | self.bitboard(by, Piece::Queen)) != 0
    }

    pub fn generate_legal_moves(self, moves: &mut MoveList) {
        let mut pseudo = MoveList::default();
        self.generate_pseudo_legal_moves(&mut pseudo);
        moves.clear();
        let moving_color = self.side_to_move;
        for chess_move in pseudo.as_slice() {
            let mut next = self;
            next.play_unchecked(*chess_move);
            if !next.in_check(moving_color) {
                moves.push(*chess_move);
            }
        }
    }

    /// Applies a move without testing whether it is legal in this position.
    ///
    /// # Panics
    ///
    /// Panics if the source square does not contain a piece of the side to move
    /// or if a castling move has an invalid king destination.
    pub fn play_unchecked(&mut self, chess_move: Move) {
        let color = self.side_to_move;
        let opponent = color.opposite();
        let from = chess_move.from();
        let to = chess_move.to();
        let piece = chess_move.piece().unwrap_or_else(|| {
            self.piece_at(from)
                .filter(|(piece_color, _)| *piece_color == color)
                .map(|(_, piece)| piece)
                .expect("unchecked move must have a moving piece")
        });

        self.pieces[piece_index(color, piece)] &= !from.bit();
        let captured_square = if chess_move.is_en_passant() {
            Square(if color == Color::White {
                to.0 - 8
            } else {
                to.0 + 8
            })
        } else {
            to
        };
        let captured = if chess_move.is_capture() {
            self.piece_at(captured_square)
                .filter(|(captured_color, _)| *captured_color == opponent)
        } else {
            None
        };
        if let Some((_, captured_piece)) = captured {
            self.pieces[piece_index(opponent, captured_piece)] &= !captured_square.bit();
        }

        if chess_move.is_castle() {
            let (rook_from, rook_to) = match to {
                Square::G1 => (Square::H1, Square::F1),
                Square::C1 => (Square::A1, Square::D1),
                Square::G8 => (Square::H8, Square::F8),
                Square::C8 => (Square::A8, Square::D8),
                _ => unreachable!("invalid castling destination"),
            };
            self.pieces[piece_index(color, Piece::Rook)] &= !rook_from.bit();
            self.pieces[piece_index(color, Piece::Rook)] |= rook_to.bit();
        }

        let placed_piece = chess_move.promotion().unwrap_or(piece);
        self.pieces[piece_index(color, placed_piece)] |= to.bit();
        self.update_castling_rights(color, piece, from, captured, captured_square);
        self.en_passant = if piece == Piece::Pawn && chess_move.0 & Move::DOUBLE_PAWN != 0 {
            if color == Color::White {
                from.0 + 8
            } else {
                from.0 - 8
            }
        } else {
            NO_SQUARE
        };
        self.halfmove_clock = if piece == Piece::Pawn || captured.is_some() {
            0
        } else {
            self.halfmove_clock.saturating_add(1)
        };
        if color == Color::Black {
            self.fullmove_number = self.fullmove_number.saturating_add(1);
        }
        self.side_to_move = opponent;
    }

    fn update_castling_rights(
        &mut self,
        color: Color,
        piece: Piece,
        from: Square,
        captured: Option<(Color, Piece)>,
        captured_square: Square,
    ) {
        if piece == Piece::King {
            self.castling.0 &= match color {
                Color::White => !(CastlingRights::WHITE_KINGSIDE | CastlingRights::WHITE_QUEENSIDE),
                Color::Black => !(CastlingRights::BLACK_KINGSIDE | CastlingRights::BLACK_QUEENSIDE),
            };
        }
        if piece == Piece::Rook {
            clear_rook_right(&mut self.castling.0, from);
        }
        if captured.is_some_and(|(_, captured_piece)| captured_piece == Piece::Rook) {
            clear_rook_right(&mut self.castling.0, captured_square);
        }
    }

    fn generate_pseudo_legal_moves(self, moves: &mut MoveList) {
        self.generate_pawns(moves);
        self.generate_leapers(Piece::Knight, moves);
        self.generate_sliders(Piece::Bishop, moves);
        self.generate_sliders(Piece::Rook, moves);
        self.generate_sliders(Piece::Queen, moves);
        self.generate_king(moves);
    }

    fn generate_pawns(self, moves: &mut MoveList) {
        let color = self.side_to_move;
        let occupied = self.occupied();
        let opponents = self.occupied_by(color.opposite());
        let mut pawns = self.bitboard(color, Piece::Pawn);
        while let Some(from) = pop_square(&mut pawns) {
            let direction = if color == Color::White { 1 } else { -1 };
            if let Some(to) = offset_square(from, 0, direction) {
                if occupied & to.bit() == 0 {
                    Self::push_pawn_move(moves, from, to, 0);
                    let start_rank = if color == Color::White { 1 } else { 6 };
                    if from.rank() == start_rank {
                        if let Some(double_to) = offset_square(from, 0, direction * 2) {
                            if occupied & double_to.bit() == 0 {
                                moves.push(Move::new(
                                    from,
                                    double_to,
                                    Piece::Pawn,
                                    None,
                                    Move::DOUBLE_PAWN,
                                ));
                            }
                        }
                    }
                }
            }
            for file_delta in [-1, 1] {
                let Some(to) = offset_square(from, file_delta, direction) else {
                    continue;
                };
                if opponents & to.bit() != 0 {
                    Self::push_pawn_move(moves, from, to, Move::CAPTURE);
                } else if self.en_passant == to.0 {
                    let captured = Square(if color == Color::White {
                        to.0 - 8
                    } else {
                        to.0 + 8
                    });
                    if self.bitboard(color.opposite(), Piece::Pawn) & captured.bit() != 0 {
                        moves.push(Move::new(
                            from,
                            to,
                            Piece::Pawn,
                            None,
                            Move::CAPTURE | Move::EN_PASSANT,
                        ));
                    }
                }
            }
        }
    }

    fn push_pawn_move(moves: &mut MoveList, from: Square, to: Square, flags: u32) {
        if matches!(to.rank(), 0 | 7) {
            for promotion in [Piece::Queen, Piece::Rook, Piece::Bishop, Piece::Knight] {
                moves.push(Move::new(from, to, Piece::Pawn, Some(promotion), flags));
            }
        } else {
            moves.push(Move::new(from, to, Piece::Pawn, None, flags));
        }
    }

    fn generate_leapers(self, piece: Piece, moves: &mut MoveList) {
        let own = self.occupied_by(self.side_to_move);
        let opponents = self.occupied_by(self.side_to_move.opposite());
        let mut pieces = self.bitboard(self.side_to_move, piece);
        while let Some(from) = pop_square(&mut pieces) {
            let mut destinations = knight_attacks(from) & !own;
            while let Some(to) = pop_square(&mut destinations) {
                let flags = if opponents & to.bit() != 0 {
                    Move::CAPTURE
                } else {
                    0
                };
                moves.push(Move::new(from, to, piece, None, flags));
            }
        }
    }

    fn generate_sliders(self, piece: Piece, moves: &mut MoveList) {
        let own = self.occupied_by(self.side_to_move);
        let opponents = self.occupied_by(self.side_to_move.opposite());
        let occupied = own | opponents;
        let mut pieces = self.bitboard(self.side_to_move, piece);
        while let Some(from) = pop_square(&mut pieces) {
            let mut destinations = sliding_moves(from, piece, occupied) & !own;
            while let Some(to) = pop_square(&mut destinations) {
                let flags = if opponents & to.bit() != 0 {
                    Move::CAPTURE
                } else {
                    0
                };
                moves.push(Move::new(from, to, piece, None, flags));
            }
        }
    }

    fn generate_king(self, moves: &mut MoveList) {
        let color = self.side_to_move;
        let own = self.occupied_by(color);
        let opponents = self.occupied_by(color.opposite());
        let king = self.bitboard(color, Piece::King);
        let from = square_from_bit(king);
        let mut destinations = king_attacks(from) & !own;
        while let Some(to) = pop_square(&mut destinations) {
            let flags = if opponents & to.bit() != 0 {
                Move::CAPTURE
            } else {
                0
            };
            moves.push(Move::new(from, to, Piece::King, None, flags));
        }
        self.generate_castles(moves);
    }

    fn generate_castles(self, moves: &mut MoveList) {
        let color = self.side_to_move;
        let opponent = color.opposite();
        let occupied = self.occupied();
        let (king, kingside, queenside) = match color {
            Color::White => (
                Square::E1,
                CastlingRights::WHITE_KINGSIDE,
                CastlingRights::WHITE_QUEENSIDE,
            ),
            Color::Black => (
                Square::E8,
                CastlingRights::BLACK_KINGSIDE,
                CastlingRights::BLACK_QUEENSIDE,
            ),
        };
        if self.bitboard(color, Piece::King) & king.bit() == 0
            || self.is_square_attacked(king, opponent)
        {
            return;
        }
        let (kingside_rook, transit, destination) = match color {
            Color::White => (Square::H1, Square::F1, Square::G1),
            Color::Black => (Square::H8, Square::F8, Square::G8),
        };
        if self.castling.contains(kingside)
            && self.bitboard(color, Piece::Rook) & kingside_rook.bit() != 0
            && occupied & (transit.bit() | destination.bit()) == 0
            && !self.is_square_attacked(transit, opponent)
            && !self.is_square_attacked(destination, opponent)
        {
            moves.push(Move::new(
                king,
                destination,
                Piece::King,
                None,
                Move::CASTLE,
            ));
        }
        let (queenside_rook, rook_gap, destination, transit) = match color {
            Color::White => (Square::A1, Square(1), Square::C1, Square::D1),
            Color::Black => (Square::A8, Square(57), Square::C8, Square::D8),
        };
        if self.castling.contains(queenside)
            && self.bitboard(color, Piece::Rook) & queenside_rook.bit() != 0
            && occupied & (rook_gap.bit() | destination.bit() | transit.bit()) == 0
            && !self.is_square_attacked(transit, opponent)
            && !self.is_square_attacked(destination, opponent)
        {
            moves.push(Move::new(
                king,
                destination,
                Piece::King,
                None,
                Move::CASTLE,
            ));
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FenError {
    MissingField,
    TooManyFields,
    InvalidBoard,
    InvalidSideToMove,
    InvalidCastlingRights,
    InvalidEnPassant,
    InvalidHalfmoveClock,
    InvalidFullmoveNumber,
    InvalidKings,
}

impl fmt::Display for FenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingField => "FEN has fewer than six fields",
            Self::TooManyFields => "FEN has more than six fields",
            Self::InvalidBoard => "invalid FEN board field",
            Self::InvalidSideToMove => "invalid FEN side-to-move field",
            Self::InvalidCastlingRights => "invalid FEN castling field",
            Self::InvalidEnPassant => "invalid FEN en-passant field",
            Self::InvalidHalfmoveClock => "invalid FEN halfmove clock",
            Self::InvalidFullmoveNumber => "invalid FEN fullmove number",
            Self::InvalidKings => "FEN must contain exactly one king per side",
        })
    }
}

impl Error for FenError {}

const fn piece_index(color: Color, piece: Piece) -> usize {
    color.index() * 6 + piece.index()
}

fn next_nonempty<'a>(fields: &mut impl Iterator<Item = &'a [u8]>) -> Option<&'a [u8]> {
    fields.find(|field| !field.is_empty())
}

fn parse_u16(bytes: &[u8]) -> Option<u16> {
    bytes.iter().try_fold(0_u16, |value, byte| {
        value
            .checked_mul(10)?
            .checked_add(u16::from(byte.checked_sub(b'0')?))
    })
}

fn piece_from_fen(byte: u8) -> Option<(Color, Piece)> {
    let color = if byte.is_ascii_uppercase() {
        Color::White
    } else {
        Color::Black
    };
    let piece = match byte.to_ascii_lowercase() {
        b'p' => Piece::Pawn,
        b'n' => Piece::Knight,
        b'b' => Piece::Bishop,
        b'r' => Piece::Rook,
        b'q' => Piece::Queen,
        b'k' => Piece::King,
        _ => return None,
    };
    Some((color, piece))
}

fn clear_rook_right(rights: &mut u8, square: Square) {
    *rights &= !match square {
        Square::A1 => CastlingRights::WHITE_QUEENSIDE,
        Square::H1 => CastlingRights::WHITE_KINGSIDE,
        Square::A8 => CastlingRights::BLACK_QUEENSIDE,
        Square::H8 => CastlingRights::BLACK_KINGSIDE,
        _ => 0,
    };
}

fn pop_square(bitboard: &mut u64) -> Option<Square> {
    if *bitboard == 0 {
        return None;
    }
    let square = square_from_bit(*bitboard);
    *bitboard &= *bitboard - 1;
    Some(square)
}

fn offset_square(square: Square, file_delta: i8, rank_delta: i8) -> Option<Square> {
    let file = i16::from(square.file()) + i16::from(file_delta);
    let rank = i16::from(square.rank()) + i16::from(rank_delta);
    if (0..8).contains(&file) && (0..8).contains(&rank) {
        let file = u8::try_from(file).expect("checked file range");
        let rank = u8::try_from(rank).expect("checked rank range");
        Some(Square(rank * 8 + file))
    } else {
        None
    }
}

fn square_from_bit(bitboard: u64) -> Square {
    Square(u8::try_from(bitboard.trailing_zeros()).expect("nonzero u64 bit index is below 64"))
}

pub(crate) fn pawn_attackers(square: Square, color: Color) -> u64 {
    match color {
        Color::White => WHITE_PAWN_ATTACKERS[usize::from(square.0)],
        Color::Black => BLACK_PAWN_ATTACKERS[usize::from(square.0)],
    }
}

pub(crate) fn knight_attacks(square: Square) -> u64 {
    KNIGHT_ATTACKS[usize::from(square.0)]
}

pub(crate) fn king_attacks(square: Square) -> u64 {
    KING_ATTACKS[usize::from(square.0)]
}

fn first_occupied_increasing(ray: u64, occupied: u64) -> u64 {
    let blockers = ray & occupied;
    blockers & blockers.wrapping_neg()
}

fn first_occupied_decreasing(ray: u64, occupied: u64) -> u64 {
    let blockers = ray & occupied;
    if blockers == 0 {
        0
    } else {
        1_u64 << (63 - blockers.leading_zeros())
    }
}

fn ray_moves_increasing(ray: u64, occupied: u64) -> u64 {
    let blocker = first_occupied_increasing(ray, occupied);
    if blocker == 0 {
        ray
    } else {
        ray & (blocker | blocker.wrapping_sub(1))
    }
}

fn ray_moves_decreasing(ray: u64, occupied: u64) -> u64 {
    let blocker = first_occupied_decreasing(ray, occupied);
    if blocker == 0 {
        ray
    } else {
        ray & !blocker.wrapping_sub(1)
    }
}

pub(crate) fn sliding_moves(square: Square, piece: Piece, occupied: u64) -> u64 {
    let square_index = usize::from(square.0);
    let diagonal = || {
        ray_moves_increasing(NORTH_EAST_RAYS[square_index], occupied)
            | ray_moves_increasing(NORTH_WEST_RAYS[square_index], occupied)
            | ray_moves_decreasing(SOUTH_EAST_RAYS[square_index], occupied)
            | ray_moves_decreasing(SOUTH_WEST_RAYS[square_index], occupied)
    };
    let orthogonal = || {
        ray_moves_increasing(EAST_RAYS[square_index], occupied)
            | ray_moves_increasing(NORTH_RAYS[square_index], occupied)
            | ray_moves_decreasing(WEST_RAYS[square_index], occupied)
            | ray_moves_decreasing(SOUTH_RAYS[square_index], occupied)
    };
    match piece {
        Piece::Bishop => diagonal(),
        Piece::Rook => orthogonal(),
        Piece::Queen => diagonal() | orthogonal(),
        Piece::Pawn | Piece::Knight | Piece::King => {
            unreachable!("only sliding pieces have ray moves")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_position_has_twenty_legal_moves() {
        assert_eq!(std::mem::size_of::<Position>(), 104);
        assert_eq!(std::mem::size_of::<Move>(), 4);
        let mut moves = MoveList::default();
        let position = Position::initial();
        position.generate_legal_moves(&mut moves);
        assert_eq!(moves.len(), 20);
        for chess_move in moves.as_slice() {
            assert_eq!(
                chess_move.piece(),
                position.piece_at(chess_move.from()).map(|(_, piece)| piece)
            );
        }
    }

    #[test]
    fn parses_fen_and_generates_castling() {
        let position = Position::from_fen(b"r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
        let mut moves = MoveList::default();
        position.generate_legal_moves(&mut moves);
        assert_eq!(moves.as_slice().iter().filter(|m| m.is_castle()).count(), 2);
    }

    fn perft(position: Position, depth: u8) -> u64 {
        if depth == 0 {
            return 1;
        }
        let mut moves = MoveList::default();
        position.generate_legal_moves(&mut moves);
        moves
            .as_slice()
            .iter()
            .map(|chess_move| {
                let mut next = position;
                next.play_unchecked(*chess_move);
                perft(next, depth - 1)
            })
            .sum()
    }

    #[test]
    fn initial_position_perft() {
        let position = Position::initial();
        assert_eq!(perft(position, 1), 20);
        assert_eq!(perft(position, 2), 400);
        assert_eq!(perft(position, 3), 8_902);
        assert_eq!(perft(position, 4), 197_281);
    }

    #[test]
    fn tactical_perft_positions_cover_special_rules() {
        let kiwipete = Position::from_fen(
            b"r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        )
        .unwrap();
        assert_eq!(perft(kiwipete, 1), 48);
        assert_eq!(perft(kiwipete, 2), 2_039);
        assert_eq!(perft(kiwipete, 3), 97_862);

        let endgame = Position::from_fen(b"8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1").unwrap();
        assert_eq!(perft(endgame, 1), 14);
        assert_eq!(perft(endgame, 2), 191);
        assert_eq!(perft(endgame, 3), 2_812);
        assert_eq!(perft(endgame, 4), 43_238);
    }

    #[test]
    fn sliding_rays_match_coordinate_walks() {
        fn reference(square: Square, piece: Piece, occupied: u64) -> u64 {
            let directions: &[(i8, i8)] = match piece {
                Piece::Bishop => &[(1, 1), (1, -1), (-1, 1), (-1, -1)],
                Piece::Rook => &[(1, 0), (-1, 0), (0, 1), (0, -1)],
                Piece::Queen => &[
                    (1, 1),
                    (1, -1),
                    (-1, 1),
                    (-1, -1),
                    (1, 0),
                    (-1, 0),
                    (0, 1),
                    (0, -1),
                ],
                Piece::Pawn | Piece::Knight | Piece::King => unreachable!(),
            };
            let mut destinations = 0_u64;
            for (file_delta, rank_delta) in directions {
                let mut current = square;
                while let Some(to) = offset_square(current, *file_delta, *rank_delta) {
                    destinations |= to.bit();
                    if occupied & to.bit() != 0 {
                        break;
                    }
                    current = to;
                }
            }
            destinations
        }

        let fixed_occupancies = [
            0,
            u64::MAX,
            0x00ff_0000_0000_ff00,
            0x8142_2418_1824_4281,
            0xaa55_aa55_55aa_55aa,
        ];
        for square_index in 0..64 {
            let square = Square(square_index);
            for piece in [Piece::Bishop, Piece::Rook, Piece::Queen] {
                for occupied in fixed_occupancies
                    .into_iter()
                    .chain((0..64).map(|bit| 1_u64 << bit))
                {
                    assert_eq!(
                        sliding_moves(square, piece, occupied),
                        reference(square, piece, occupied),
                        "{piece:?} from {square} with occupancy {occupied:#018x}"
                    );
                }
            }
        }
    }
}
