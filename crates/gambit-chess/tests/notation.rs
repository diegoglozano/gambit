use gambit_chess::Position;

#[test]
fn fen_roundtrips_complete_state() {
    for fen in [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 42 63",
        "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 9",
        "7k/5KP1/8/8/8/8/8/8 w - - 0 72",
    ] {
        let position = Position::from_fen(fen.as_bytes()).unwrap();
        assert_eq!(position.to_fen(), fen);
        assert_eq!(
            Position::from_fen(position.to_fen().as_bytes()).unwrap(),
            position
        );
    }
}

#[test]
fn replay_exports_special_moves_and_clocks() {
    let mut position = Position::initial();
    assert_eq!(position.play_san(b"e4").unwrap().to_uci(), "e2e4");
    assert_eq!(
        position.to_fen(),
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1"
    );
    position.play_san(b"Nf6").unwrap();
    assert_eq!(
        position.to_fen(),
        "rnbqkb1r/pppppppp/5n2/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 1 2"
    );
    for (fen, san, uci, after) in [
        (
            "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 9",
            "exd6",
            "e5d6",
            "4k3/8/3P4/8/8/8/8/4K3 b - - 0 9",
        ),
        (
            "4k3/8/8/8/8/8/8/R3K2R w KQ - 3 9",
            "O-O",
            "e1g1",
            "4k3/8/8/8/8/8/8/R4RK1 b - - 4 9",
        ),
        (
            "7k/5KP1/8/8/8/8/8/8 w - - 0 72",
            "g8=N",
            "g7g8n",
            "6Nk/5K2/8/8/8/8/8/8 b - - 0 72",
        ),
    ] {
        let mut position = Position::from_fen(fen.as_bytes()).unwrap();
        assert_eq!(position.play_san(san.as_bytes()).unwrap().to_uci(), uci);
        assert_eq!(position.to_fen(), after);
    }
}
