use gambit_chess::Color;
use gambit_coaching::{InputError, ReviewGame};

const GAME: &str = "[White \"Alice\"]\n[Black \"Bob\"]\n1. d4 d5 2. Bf4 Nf6 3. e3 e6 *";

#[test]
fn shared_ply_and_both_player_perspectives_are_explicit() {
    for (player, color, plies, moves) in [
        (" Alice ", Color::White, vec![5], vec!["e2e3"]),
        ("bOB", Color::Black, vec![6], vec!["e7e6"]),
    ] {
        let game = ReviewGame::parse(GAME.as_bytes(), player).unwrap();
        assert_eq!(game.player(), color);
        let decisions = game.decisions(4).collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(decisions.iter().map(|d| d.ply).collect::<Vec<_>>(), plies);
        assert_eq!(
            decisions
                .iter()
                .map(|d| d.played_uci.as_str())
                .collect::<Vec<_>>(),
            moves
        );
        for decision in decisions {
            assert_eq!(decision.before.position().side_to_move(), color);
            assert_eq!(decision.after.position().side_to_move(), color.opposite());
            assert_eq!(decision.before.moves().len(), decision.ply - 1);
            assert_eq!(decision.after.moves().len(), decision.ply);
        }
        assert_eq!(game.decisions(100).count(), 0);
    }
}

#[test]
fn repeats_are_preserved_and_variations_are_excluded() {
    let game = ReviewGame::parse(
        b"[White \"A\"]\n[Black \"B\"]\n1. Nf3 (1. e4 e5) Nf6 2. Ng1 Ng8 3. Nf3 Nf6 *",
        "A",
    )
    .unwrap();
    assert_eq!(game.mainline().len(), 6);
    let decision = game.decisions(4).next().unwrap().unwrap();
    assert_eq!(decision.before.moves(), ["g1f3", "g8f6", "f3g1", "f6g8"]);
    assert_eq!(decision.played_san, "Nf3");
}

#[test]
fn setup_games_count_mainline_plies_not_fen_move_numbers() {
    let game = ReviewGame::parse(b"[White \"A\"]\n[Black \"B\"]\n[SetUp \"1\"]\n[FEN \"4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 9\"]\n9. exd6 Kd7 *", "A").unwrap();
    let decision = game.decisions(0).next().unwrap().unwrap();
    assert_eq!(decision.ply, 1);
    assert_eq!(decision.played_uci, "e5d6");
    assert!(decision.before.moves().is_empty());
    assert_eq!(
        decision.after.position().to_fen(),
        "4k3/8/3P4/8/8/8/8/4K3 b - - 0 9"
    );
}

#[test]
fn rejects_unsafe_or_ambiguous_inputs_without_an_engine() {
    assert_eq!(
        ReviewGame::parse(GAME.as_bytes(), " ").unwrap_err(),
        InputError::PlayerRequired
    );
    assert_eq!(
        ReviewGame::parse(GAME.as_bytes(), "Nobody").unwrap_err(),
        InputError::PlayerMissingOrAmbiguous
    );
    for pgn in [
        "[White \"A\"]\n[Black \"A\"]\n1.e4 *",
        "[White \"A\"]\n[Black \"B\"]\n[White \"B\"]\n1.e4 *",
        "[White \"A\"]\n[Black \"B\"]\n[SetUp \"1\"]\n1.e4 *",
        "[White \"A\"]\n[Black \"B\"]\n[Variant \"Atomic\"]\n1.e4 *",
        "[White \"A\"]\n[Black \"B\"]\n1.e5 *",
        "[White \"A\"]\n[Black \"B\"]\n1.e4",
        "",
    ] {
        assert!(ReviewGame::parse(pgn.as_bytes(), "A").is_err(), "{pgn}");
    }
    assert_eq!(
        ReviewGame::parse(format!("{GAME}\n{GAME}").as_bytes(), "Alice").unwrap_err(),
        InputError::ExpectedOneGame
    );
    assert_eq!(
        ReviewGame::parse(&vec![b' '; ReviewGame::MAX_PGN_BYTES + 1], "A").unwrap_err(),
        InputError::TooLarge
    );
    let long = format!(
        "[White \"A\"]\n[Black \"B\"]\n{} *",
        "Nf3 Nf6 Ng1 Ng8 ".repeat(257)
    );
    assert_eq!(
        ReviewGame::parse(long.as_bytes(), "A").unwrap_err(),
        InputError::TooLarge
    );
}

#[test]
fn empty_and_short_games_have_no_forced_lesson() {
    let game = ReviewGame::parse(b"[White \"A\"]\n[Black \"B\"]\n*", "A").unwrap();
    assert!(game.mainline().is_empty());
    assert_eq!(game.decisions(0).count(), 0);
}
