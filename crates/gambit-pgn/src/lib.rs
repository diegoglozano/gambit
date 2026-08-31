//! A zero-copy, pull-based parser for Portable Game Notation (PGN).
//!
//! The parser deliberately handles PGN structure, not chess legality. SAN moves
//! are returned as borrowed byte slices so a later chess layer can validate or
//! execute them without this crate allocating an intermediate game tree.

mod parser;

pub use parser::{
    Comment, CommentKind, ErrorKind, Event, Nag, Outcome, ParseError, Parser, ParserOptions, Span,
    Tag, Token,
};
