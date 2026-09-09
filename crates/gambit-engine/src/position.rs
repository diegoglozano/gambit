use gambit_chess::{MoveList, Position};

use crate::Error;

/// A standard position and its legally replayed history. Fields are private so
/// callers cannot accidentally pair a board with a different UCI history.
#[derive(Clone, Debug)]
pub struct GamePosition {
    initial_fen: String,
    moves: Vec<String>,
    current: Position,
}

impl Default for GamePosition {
    fn default() -> Self {
        Self {
            initial_fen: Position::initial().to_fen(),
            moves: Vec::new(),
            current: Position::initial(),
        }
    }
}

impl GamePosition {
    /// Keep outbound requests bounded, including promotion suffixes.
    pub const MAX_PLIES: usize = 1024;

    /// Begin history at a setup position. Earlier repetition is not inferred.
    ///
    /// # Errors
    /// Rejects invalid FENs and oversized or multiline protocol input.
    pub fn from_fen(fen: &str) -> Result<Self, Error> {
        if fen.len() > 128 || fen.contains(['\n', '\r']) {
            return Err(Error::InvalidPosition);
        }
        let current = Position::from_fen(fen.as_bytes()).map_err(|_| Error::InvalidPosition)?;
        Ok(Self {
            initial_fen: current.to_fen(),
            moves: Vec::new(),
            current,
        })
    }

    #[must_use]
    pub fn position(&self) -> Position {
        self.current
    }

    #[must_use]
    pub fn initial_fen(&self) -> &str {
        &self.initial_fen
    }

    #[must_use]
    pub fn moves(&self) -> &[String] {
        &self.moves
    }

    /// Append exactly one legal standard UCI move, including promotion choice.
    /// Failed attempts leave both board and history unchanged.
    ///
    /// # Errors
    /// Rejects illegal/malformed moves and histories exceeding `MAX_PLIES`.
    pub fn play_uci(&mut self, notation: &str) -> Result<(), Error> {
        if self.moves.len() >= Self::MAX_PLIES || !crate::uci_move(notation) {
            return Err(Error::InvalidPosition);
        }
        let mut legal = MoveList::default();
        self.current.generate_legal_moves(&mut legal);
        let chess_move = legal
            .as_slice()
            .iter()
            .find(|m| m.to_uci() == notation)
            .copied()
            .ok_or(Error::InvalidPosition)?;
        self.current.play_unchecked(chess_move);
        self.moves.push(notation.to_owned());
        Ok(())
    }

    pub(crate) fn command(&self) -> String {
        let mut command = format!("position fen {}", self.initial_fen);
        if !self.moves.is_empty() {
            command.push_str(" moves ");
            command.push_str(&self.moves.join(" "));
        }
        debug_assert!(command.len() < crate::MAX_LINE);
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repetition_history_survives_returning_to_the_same_board() {
        let mut game = GamePosition::default();
        for m in ["g1f3", "g8f6", "f3g1", "f6g8"].repeat(2) {
            game.play_uci(m).unwrap();
        }
        assert!(game.position().same_position(Position::initial()));
        assert_eq!(game.moves().len(), 8);
        assert!(
            game.command()
                .ends_with("moves g1f3 g8f6 f3g1 f6g8 g1f3 g8f6 f3g1 f6g8")
        );
        assert_ne!(
            game.command(),
            GamePosition::from_fen(&game.position().to_fen())
                .unwrap()
                .command()
        );
    }

    #[test]
    fn legal_special_moves_replay_exactly() {
        for (fen, notation, expected) in [
            (
                "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 9",
                "e5d6",
                "4k3/8/3P4/8/8/8/8/4K3 b - - 0 9",
            ),
            (
                "4k3/8/8/8/8/8/8/4K2R w K - 0 1",
                "e1g1",
                "4k3/8/8/8/8/8/8/5RK1 b - - 1 1",
            ),
            (
                "4k3/P7/8/8/8/8/8/4K3 w - - 0 1",
                "a7a8n",
                "N3k3/8/8/8/8/8/8/4K3 b - - 0 1",
            ),
        ] {
            let mut game = GamePosition::from_fen(fen).unwrap();
            game.play_uci(notation).unwrap();
            assert_eq!(game.position().to_fen(), expected);
            assert_eq!(game.initial_fen(), fen);
        }
    }

    #[test]
    fn invalid_moves_are_atomic_and_input_is_bounded() {
        let mut game = GamePosition::default();
        for bad in ["e2e5", "e7e5", "e2e4q", "0000", "e2e4\nquit", "e2e4 e7e5"] {
            assert!(game.play_uci(bad).is_err());
            assert_eq!(game.position(), Position::initial());
            assert!(game.moves().is_empty());
        }
        for m in ["g1f3", "g8f6", "f3g1", "f6g8"].repeat(GamePosition::MAX_PLIES / 4) {
            game.play_uci(m).unwrap();
        }
        let before = game.command();
        assert!(game.play_uci("g1f3").is_err());
        assert_eq!(game.command(), before);
        assert!(before.len() < crate::MAX_LINE);
    }
}
