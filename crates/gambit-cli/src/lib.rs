//! Reusable application services for Gambit's command-line and desktop clients.

#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate
)]

pub mod collection;
pub mod index;
pub mod lichess;
pub mod query;
pub mod sync;
