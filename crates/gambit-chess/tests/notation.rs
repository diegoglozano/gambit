use gambit_chess::{Move, MoveList, Position};

#[test]
fn formats_legal_moves_with_minimal_disambiguation_and_check_suffixes() {
    for (fen, input, expected) in [
        ("4k3/8/8/8/8/2N1N3/8/4K3 w - - 0 1", "Ncd5", "Ncd5"),
        ("7k/2N5/8/8/8/2N5/8/4K3 w - - 0 1", "N3d5", "N3d5"),
        ("7k/2N5/8/8/8/2N1N3/8/4K3 w - - 0 1", "Nc3d5", "Nc3d5"),
        ("4k3/8/8/8/1b6/2N1N3/8/4K3 w - - 0 1", "Ned5", "Nd5"),
        ("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 9", "exd6", "exd6"),
        ("5k2/8/8/8/8/8/8/4K2R w K - 0 1", "O-O", "O-O+"),
        ("r3k3/8/8/8/8/8/8/7K b q - 0 1", "0-0-0", "O-O-O"),
        ("7k/5KP1/8/8/8/8/8/8 w - - 0 72", "g8=Q", "g8=Q#"),
        ("7k/5KP1/8/8/8/8/8/8 w - - 0 72", "g8=N", "g8=N"),
        ("7k/8/6K1/8/8/8/8/5Q2 w - - 0 1", "Qf7", "Qf7"),
    ] {
        let position = Position::from_fen(fen.as_bytes()).unwrap();
        let mut next = position;
        let chess_move = next.play_san(input.as_bytes()).unwrap();
        assert_eq!(position.to_san(chess_move).unwrap(), expected);
        let mut replay = position;
        assert_eq!(replay.play_san(expected.as_bytes()).unwrap(), chess_move);
        assert_eq!(replay, next);
    }
}

#[test]
fn every_generated_move_roundtrips_through_san() {
    for fen in [
        Position::initial().to_fen(),
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1".into(),
        "4k3/P7/8/3pP3/8/8/8/R3K2R w KQ d6 0 1".into(),
    ] {
        let position = Position::from_fen(fen.as_bytes()).unwrap();
        let mut moves = MoveList::default();
        position.generate_legal_moves(&mut moves);
        for &chess_move in moves.as_slice() {
            let san = position.to_san(chess_move).unwrap();
            let mut replay = position;
            assert_eq!(
                replay.play_san(san.as_bytes()).unwrap(),
                chess_move,
                "{san}"
            );
        }
    }
    let position = Position::initial();
    assert!(position.to_san(Move::default()).is_err());
    let mut next = position;
    let old_move = next.play_san(b"e4").unwrap();
    assert!(next.to_san(old_move).is_err());
}

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
