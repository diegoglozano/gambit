use gambit_chess::{Color, Position};
use gambit_engine::GamePosition;
use gambit_pgn::{Event, GameReader, GameReaderOptions, Parser, ParserOptions};
use std::io::Read;

#[derive(Debug)]
pub struct Decision {
    pub ply: usize,
    pub played: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug)]
pub struct Game {
    pub history: GamePosition,
    pub plies: usize,
    pub player: Color,
    pub decisions: Vec<Decision>,
}

pub fn workload(reader: impl Read, player: &str, shared_ply: usize) -> Result<Vec<Game>, String> {
    if player.trim().is_empty() {
        return Err("an explicit player is required".into());
    }
    let mut reader = GameReader::with_options(
        reader,
        GameReaderOptions {
            max_game_bytes: 256 * 1024,
            ..GameReaderOptions::default()
        },
    );
    let mut games = Vec::new();
    while let Some(bytes) = reader.read_game().map_err(|e| e.to_string())? {
        if games.len() == 6 {
            return Err("profile input must contain exactly six games".into());
        }
        games.push(
            replay(bytes, player, shared_ply)
                .map_err(|e| format!("game {}: {e}", games.len() + 1))?,
        );
    }
    if games.len() != 6 {
        return Err("profile input must contain exactly six games".into());
    }
    Ok(games)
}

fn replay(bytes: &[u8], player: &str, shared_ply: usize) -> Result<Game, String> {
    let mut white = None;
    let mut black = None;
    let mut fen = None;
    let mut variant = None;
    let mut selected = None;
    let mut position = Position::initial();
    let mut history = GamePosition::default();
    let mut plies = 0;
    let mut depth = 0;
    let mut decisions = Vec::new();
    for event in Parser::with_options(bytes, ParserOptions::STRICT) {
        match event.map_err(|e| e.to_string())? {
            Event::Tag(tag) => {
                let target = match tag.name() {
                    b"White" => &mut white,
                    b"Black" => &mut black,
                    b"FEN" => &mut fen,
                    b"Variant" => &mut variant,
                    _ => continue,
                };
                if target.is_some() {
                    return Err("duplicate identity/position tag".into());
                }
                *target =
                    Some(String::from_utf8(tag.value().into_owned()).map_err(|e| e.to_string())?);
            }
            Event::MovetextStart { .. } => {
                if variant
                    .as_deref()
                    .is_some_and(|v| !v.eq_ignore_ascii_case("standard"))
                {
                    return Err("only standard games are supported".into());
                }
                let matches = |name: &Option<String>| {
                    name.as_deref()
                        .is_some_and(|n| n.trim().eq_ignore_ascii_case(player.trim()))
                };
                selected = Some(match (matches(&white), matches(&black)) {
                    (true, false) => Color::White,
                    (false, true) => Color::Black,
                    _ => return Err("player missing or ambiguous".into()),
                });
                if let Some(fen) = &fen {
                    position = Position::from_fen(fen.as_bytes()).map_err(|e| e.to_string())?;
                    history = GamePosition::from_fen(fen).map_err(|e| e.to_string())?;
                }
            }
            Event::VariationStart(_) => depth += 1,
            Event::VariationEnd(_) => depth -= 1,
            Event::San(token) if depth == 0 => {
                if plies == 1024 {
                    return Err("profile game exceeds 1024 plies".into());
                }
                let include = plies >= shared_ply && Some(position.side_to_move()) == selected;
                let before = position.to_fen();
                let chess_move = position
                    .play_san(token.as_bytes())
                    .map_err(|e| e.to_string())?;
                history
                    .play_uci(&chess_move.to_uci())
                    .map_err(|e| e.to_string())?;
                plies += 1;
                if include {
                    decisions.push(Decision {
                        ply: plies,
                        played: chess_move.to_uci(),
                        before,
                        after: position.to_fen(),
                    });
                }
            }
            _ => {}
        }
    }
    if decisions.is_empty() {
        return Err("no player decisions after the requested shared ply".into());
    }
    Ok(Game {
        history,
        plies,
        player: selected.ok_or("missing player")?,
        decisions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> &'static [u8] {
        include_bytes!("../../../../benchmarks/engine/kasparov-deep-blue-1997.pgn")
    }

    #[test]
    fn six_full_games_include_both_player_perspectives() {
        let games = workload(fixture(), "Garry Kasparov", 8).unwrap();
        assert_eq!(
            games.iter().map(|g| g.plies).collect::<Vec<_>>(),
            [89, 89, 95, 111, 98, 37]
        );
        assert_eq!(
            games.iter().map(|g| g.decisions.len()).collect::<Vec<_>>(),
            [41, 40, 44, 51, 45, 14]
        );
        assert_eq!(games[0].player, Color::White);
        assert_eq!(games[1].player, Color::Black);
        assert_eq!(games[0].decisions[0].ply, 9);
        assert_eq!(games[1].decisions[0].ply, 10);
        for game in games {
            assert_eq!(game.history.moves().len(), game.plies);
            for decision in game.decisions {
                let before = Position::from_fen(decision.before.as_bytes()).unwrap();
                let after = Position::from_fen(decision.after.as_bytes()).unwrap();
                assert_eq!(before.side_to_move(), game.player);
                assert_eq!(after.side_to_move(), game.player.opposite());
            }
        }
    }

    #[test]
    fn rejects_incomplete_or_ambiguous_workloads() {
        assert!(workload(fixture(), "Nobody", 8).is_err());
        assert!(workload(fixture(), " ", 8).is_err());
        assert!(workload(fixture(), "Garry Kasparov", 200).is_err());
        assert!(workload(&b""[..], "Garry Kasparov", 8).is_err());
        let mut seven = fixture().to_vec();
        seven.extend_from_slice(fixture());
        assert!(workload(&seven[..], "Garry Kasparov", 8).is_err());
    }

    #[test]
    fn ignores_variations_and_honors_setup_fen() {
        let pgn = b"[White \"A\"]\n[Black \"B\"]\n[FEN \"4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 9\"]\n[Result \"*\"]\n9. exd6 (9. Kf2) Kd7 *";
        let game = replay(pgn, "A", 0).unwrap();
        assert_eq!(game.plies, 2);
        assert_eq!(game.history.moves(), ["e5d6", "e8d7"]);
        assert_eq!(game.decisions[0].played, "e5d6");
        assert_eq!(game.decisions[0].after, "4k3/8/3P4/8/8/8/8/4K3 b - - 0 9");
        assert!(replay(b"[White \"A\"]\n[Black \"A\"]\n1.e4 *", "A", 0).is_err());
        assert!(
            replay(
                b"[White \"A\"]\n[Black \"B\"]\n[Variant \"Atomic\"]\n1.e4 *",
                "A",
                0
            )
            .is_err()
        );
    }
}
