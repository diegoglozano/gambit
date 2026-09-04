use std::fmt;
use std::time::Duration;

use crate::query::{PlayerColor, QueryOptions};

const API_ROOT: &str = "https://lichess.org/api/games/user";
const GAME_EXPORT_ROOT: &str = "https://lichess.org/game/export";
const EARLIEST_LICHESS_TIMESTAMP: i64 = 1_356_998_400_070;

#[derive(Debug)]
pub struct UserGamesRequest<'a> {
    pub username: &'a str,
    pub maximum_games: Option<u32>,
    pub options: &'a QueryOptions,
    pub since_timestamp: Option<i64>,
    pub until_timestamp: Option<i64>,
    pub include_ongoing: bool,
    pub oldest_first: bool,
}

#[derive(Debug)]
pub enum LichessError {
    UserNotFound(String),
    GameNotFound(String),
    Unauthorized,
    RateLimited,
    HttpStatus(u16),
    Request(ureq::Error),
}

impl fmt::Display for LichessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UserNotFound(username) => write!(formatter, "user {username:?} was not found"),
            Self::GameNotFound(game_id) => write!(formatter, "game {game_id:?} was not found"),
            Self::Unauthorized => write!(
                formatter,
                "Lichess rejected LICHESS_TOKEN; check the token or unset it for public access"
            ),
            Self::RateLimited => write!(
                formatter,
                "Lichess rate limit reached; wait at least 60 seconds and try again"
            ),
            Self::HttpStatus(status) => {
                write!(
                    formatter,
                    "Lichess returned unexpected HTTP status {status}"
                )
            }
            Self::Request(error) => write!(formatter, "Lichess request failed: {error}"),
        }
    }
}

pub fn user_games(
    request: &UserGamesRequest<'_>,
    token: Option<&str>,
) -> Result<ureq::http::Response<ureq::Body>, LichessError> {
    user_games_from(API_ROOT, request, token)
}

pub fn game(
    game_id: &str,
    token: Option<&str>,
) -> Result<ureq::http::Response<ureq::Body>, LichessError> {
    game_from(GAME_EXPORT_ROOT, game_id, token)
}

fn user_games_from(
    api_root: &str,
    request: &UserGamesRequest<'_>,
    token: Option<&str>,
) -> Result<ureq::http::Response<ureq::Body>, LichessError> {
    let endpoint = format!("{}/{}", api_root.trim_end_matches('/'), request.username);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .build()
        .into();
    let mut call = agent
        .get(endpoint)
        .header("Accept", "application/x-chess-pgn")
        .header(
            "User-Agent",
            concat!(
                "gambit/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/diegoglozano/gambit)"
            ),
        )
        .query("tags", "true")
        .query("moves", "true")
        .query("clocks", "false")
        .query("evals", "false")
        .query("opening", "false")
        .query("literate", "false");

    let since;
    if let Some(timestamp) = request.since_timestamp.or_else(|| {
        request
            .options
            .since
            .map(|date| date_to_lichess_timestamp(date, false))
    }) {
        since = timestamp.max(EARLIEST_LICHESS_TIMESTAMP).to_string();
        call = call.query("since", &since);
    }
    let until;
    if let Some(timestamp) = request.until_timestamp.or_else(|| {
        request
            .options
            .until
            .map(|date| date_to_lichess_timestamp(date, true))
    }) {
        until = timestamp.max(EARLIEST_LICHESS_TIMESTAMP).to_string();
        call = call.query("until", &until);
    }
    let maximum_games;
    if let Some(maximum) = request.maximum_games {
        maximum_games = maximum.to_string();
        call = call.query("max", &maximum_games);
    }
    if let Some(opponent) = request.options.opponent.as_deref() {
        call = call.query("vs", opponent);
    }
    if let Some(color) = request.options.color {
        call = call.query(
            "color",
            match color {
                PlayerColor::White => "white",
                PlayerColor::Black => "black",
            },
        );
    }
    if request.include_ongoing {
        call = call.query("ongoing", "true");
    }
    if request.oldest_first {
        call = call.query("sort", "dateAsc");
    }
    if let Some(token) = token {
        call = call.header("Authorization", format!("Bearer {token}"));
    }

    call.call().map_err(|error| match error {
        ureq::Error::StatusCode(401 | 403) => LichessError::Unauthorized,
        ureq::Error::StatusCode(404) => LichessError::UserNotFound(request.username.to_owned()),
        ureq::Error::StatusCode(429) => LichessError::RateLimited,
        ureq::Error::StatusCode(status) => LichessError::HttpStatus(status),
        error => LichessError::Request(error),
    })
}

fn game_from(
    game_export_root: &str,
    game_id: &str,
    token: Option<&str>,
) -> Result<ureq::http::Response<ureq::Body>, LichessError> {
    let endpoint = format!("{}/{}", game_export_root.trim_end_matches('/'), game_id);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .build()
        .into();
    let mut call = agent
        .get(endpoint)
        .header("Accept", "application/x-chess-pgn")
        .header(
            "User-Agent",
            concat!(
                "gambit/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/diegoglozano/gambit)"
            ),
        )
        .query("moves", "true")
        .query("tags", "true")
        .query("clocks", "false")
        .query("evals", "false")
        .query("opening", "false")
        .query("literate", "false");
    if let Some(token) = token {
        call = call.header("Authorization", format!("Bearer {token}"));
    }
    call.call().map_err(|error| match error {
        ureq::Error::StatusCode(401 | 403) => LichessError::Unauthorized,
        ureq::Error::StatusCode(404) => LichessError::GameNotFound(game_id.to_owned()),
        ureq::Error::StatusCode(429) => LichessError::RateLimited,
        ureq::Error::StatusCode(status) => LichessError::HttpStatus(status),
        error => LichessError::Request(error),
    })
}

fn date_to_unix_milliseconds(date: u32, end_of_day: bool) -> i64 {
    let year = i64::from(date / 10_000);
    let month = i64::from(date / 100 % 100);
    let day = i64::from(date % 100);
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days_since_epoch = era * 146_097 + day_of_era - 719_468;
    days_since_epoch * 86_400_000 + if end_of_day { 86_399_999 } else { 0 }
}

fn date_to_lichess_timestamp(date: u32, end_of_day: bool) -> i64 {
    date_to_unix_milliseconds(date, end_of_day).max(EARLIEST_LICHESS_TIMESTAMP)
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use super::*;

    #[test]
    fn converts_inclusive_date_bounds_to_unix_milliseconds() {
        assert_eq!(date_to_unix_milliseconds(19_700_101, false), 0);
        assert_eq!(date_to_unix_milliseconds(19_700_101, true), 86_399_999);
        assert_eq!(
            date_to_unix_milliseconds(20_000_229, false),
            951_782_400_000
        );
        assert_eq!(
            date_to_unix_milliseconds(20_260_904, false),
            1_788_480_000_000
        );
        assert_eq!(
            date_to_lichess_timestamp(19_700_101, false),
            EARLIEST_LICHESS_TIMESTAMP
        );
    }

    #[test]
    fn streams_pgn_and_pushes_safe_filters_to_lichess() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = vec![0; 8192];
            let bytes = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..bytes]);
            assert!(request.starts_with("GET /diegoglozano?"));
            assert!(request.contains("since=1767225600000"));
            assert!(request.contains("until=1798761599999"));
            assert!(request.contains("max=25"));
            assert!(request.contains("vs=Some%20Opponent"));
            assert!(request.contains("color=black"));
            assert!(request.contains("ongoing=true"));
            assert!(request.contains("sort=dateAsc"));
            assert!(request.contains("accept: application/x-chess-pgn"));
            assert!(request.contains("authorization: Bearer secret-token"));
            let pgn = b"[White \"A\"]\n[Black \"B\"]\n\n1. e4 *\n";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/x-chess-pgn\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                pgn.len()
            )
            .unwrap();
            stream.write_all(pgn).unwrap();
        });

        let options = QueryOptions {
            opponent: Some(String::from("Some Opponent")),
            color: Some(PlayerColor::Black),
            since: Some(20_260_101),
            until: Some(20_261_231),
            ..QueryOptions::default()
        };
        let request = UserGamesRequest {
            username: "diegoglozano",
            maximum_games: Some(25),
            options: &options,
            since_timestamp: None,
            until_timestamp: None,
            include_ongoing: true,
            oldest_first: true,
        };
        let mut response =
            user_games_from(&format!("http://{address}"), &request, Some("secret-token")).unwrap();
        let mut body = String::new();
        response
            .body_mut()
            .as_reader()
            .read_to_string(&mut body)
            .unwrap();
        assert!(body.contains("1. e4 *"));
        server.join().unwrap();
    }

    #[test]
    fn exports_one_game_with_minimal_pgn_options() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = vec![0; 4096];
            let bytes = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..bytes]);
            assert!(request.starts_with("GET /AbCd1234?"));
            assert!(request.contains("clocks=false"));
            assert!(request.contains("evals=false"));
            let pgn = b"[Site \"https://lichess.org/AbCd1234\"]\n\n1. e4 *\n";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                pgn.len()
            )
            .unwrap();
            stream.write_all(pgn).unwrap();
        });
        let mut response = game_from(&format!("http://{address}"), "AbCd1234", None).unwrap();
        let mut body = String::new();
        response
            .body_mut()
            .as_reader()
            .read_to_string(&mut body)
            .unwrap();
        assert!(body.contains("AbCd1234"));
        server.join().unwrap();
    }
}
