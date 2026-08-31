//! Compact chess positions, legal move generation, and SAN execution.

mod position;
mod san;

pub use position::{CastlingRights, Color, FenError, Move, MoveList, Piece, Position, Square};
pub use san::{SanError, SanErrorKind};
