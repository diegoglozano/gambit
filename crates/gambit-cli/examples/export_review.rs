//! Export six explicitly selected library games to stdout without modifying it.
use std::{
    collections::HashSet,
    io::{self, Write},
    path::PathBuf,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 7 {
        return Err("usage: export_review LIBRARY ID1 ID2 ID3 ID4 ID5 ID6".into());
    }
    let library = PathBuf::from(&args[0]);
    let mut ids = HashSet::new();
    let mut games = Vec::new();
    for arg in args.iter().skip(1) {
        let id: i64 = arg.to_str().ok_or("invalid game ID")?.parse()?;
        if !ids.insert(id) {
            return Err("review games must be distinct".into());
        }
        games.push(gambit::index::game(&library, id)?.pgn);
    }
    let mut out = io::stdout().lock();
    for pgn in games {
        writeln!(out, "{}\n", pgn.trim_end())?;
    }
    Ok(())
}
