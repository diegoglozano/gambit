use std::io::Read;
use std::time::Instant;

use gambit_pgn::{
    Event, IncrementalParser, IncrementalParserOptions, Outcome, ParserOptions, StreamParseError,
    Tag,
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatsOptions {
    pub require_outcome: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatsStatus {
    Valid,
    Invalid,
    Error,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ResultCounts {
    pub white_wins: u64,
    pub black_wins: u64,
    pub draws: u64,
    pub unfinished: u64,
}

impl ResultCounts {
    fn record(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::WhiteWins => self.white_wins += 1,
            Outcome::BlackWins => self.black_wins += 1,
            Outcome::Draw => self.draws += 1,
            Outcome::Unknown => self.unfinished += 1,
        }
    }

    pub fn add(&mut self, other: Self) {
        self.white_wins += other.white_wins;
        self.black_wins += other.black_wins;
        self.draws += other.draws;
        self.unfinished += other.unfinished;
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct GameLengthDistribution {
    pub zero: u64,
    pub from_1_to_20: u64,
    pub from_21_to_40: u64,
    pub from_41_to_60: u64,
    pub from_61_to_80: u64,
    pub from_81_to_120: u64,
    pub from_121_to_160: u64,
    pub at_least_161: u64,
}

impl GameLengthDistribution {
    fn record(&mut self, plies: u64) {
        match plies {
            0 => self.zero += 1,
            1..=20 => self.from_1_to_20 += 1,
            21..=40 => self.from_21_to_40 += 1,
            41..=60 => self.from_41_to_60 += 1,
            61..=80 => self.from_61_to_80 += 1,
            81..=120 => self.from_81_to_120 += 1,
            121..=160 => self.from_121_to_160 += 1,
            _ => self.at_least_161 += 1,
        }
    }

    pub fn add(&mut self, other: Self) {
        self.zero += other.zero;
        self.from_1_to_20 += other.from_1_to_20;
        self.from_21_to_40 += other.from_21_to_40;
        self.from_41_to_60 += other.from_41_to_60;
        self.from_61_to_80 += other.from_61_to_80;
        self.from_81_to_120 += other.from_81_to_120;
        self.from_121_to_160 += other.from_121_to_160;
        self.at_least_161 += other.at_least_161;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
#[allow(clippy::struct_field_names)]
pub struct GameLengthStats {
    pub minimum_plies: Option<u64>,
    pub average_plies: f64,
    pub maximum_plies: Option<u64>,
    pub distribution: GameLengthDistribution,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct RatingDistribution {
    pub under_1000: u64,
    pub from_1000_to_1199: u64,
    pub from_1200_to_1399: u64,
    pub from_1400_to_1599: u64,
    pub from_1600_to_1799: u64,
    pub from_1800_to_1999: u64,
    pub from_2000_to_2199: u64,
    pub from_2200_to_2399: u64,
    pub at_least_2400: u64,
}

impl RatingDistribution {
    fn record(&mut self, rating: u32) {
        match rating {
            0..=999 => self.under_1000 += 1,
            1000..=1199 => self.from_1000_to_1199 += 1,
            1200..=1399 => self.from_1200_to_1399 += 1,
            1400..=1599 => self.from_1400_to_1599 += 1,
            1600..=1799 => self.from_1600_to_1799 += 1,
            1800..=1999 => self.from_1800_to_1999 += 1,
            2000..=2199 => self.from_2000_to_2199 += 1,
            2200..=2399 => self.from_2200_to_2399 += 1,
            _ => self.at_least_2400 += 1,
        }
    }

    fn add(&mut self, other: Self) {
        self.under_1000 += other.under_1000;
        self.from_1000_to_1199 += other.from_1000_to_1199;
        self.from_1200_to_1399 += other.from_1200_to_1399;
        self.from_1400_to_1599 += other.from_1400_to_1599;
        self.from_1600_to_1799 += other.from_1600_to_1799;
        self.from_1800_to_1999 += other.from_1800_to_1999;
        self.from_2000_to_2199 += other.from_2000_to_2199;
        self.from_2200_to_2399 += other.from_2200_to_2399;
        self.at_least_2400 += other.at_least_2400;
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct TimeControlStats {
    pub sudden_death: u64,
    pub increment: u64,
    pub moves_per_period: u64,
    pub multi_stage: u64,
    pub hourglass: u64,
    pub unknown: u64,
    pub unlimited: u64,
    pub invalid: u64,
    pub missing: u64,
}

impl TimeControlStats {
    fn record(&mut self, kind: TimeControlKind) {
        match kind {
            TimeControlKind::SuddenDeath => self.sudden_death += 1,
            TimeControlKind::Increment => self.increment += 1,
            TimeControlKind::MovesPerPeriod => self.moves_per_period += 1,
            TimeControlKind::MultiStage => self.multi_stage += 1,
            TimeControlKind::Hourglass => self.hourglass += 1,
            TimeControlKind::Unknown => self.unknown += 1,
            TimeControlKind::Unlimited => self.unlimited += 1,
            TimeControlKind::Invalid => self.invalid += 1,
            TimeControlKind::Missing => self.missing += 1,
        }
    }

    pub fn add(&mut self, other: Self) {
        self.sudden_death += other.sudden_death;
        self.increment += other.increment;
        self.moves_per_period += other.moves_per_period;
        self.multi_stage += other.multi_stage;
        self.hourglass += other.hourglass;
        self.unknown += other.unknown;
        self.unlimited += other.unlimited;
        self.invalid += other.invalid;
        self.missing += other.missing;
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct HeaderCoverage {
    pub event: u64,
    pub site: u64,
    pub date: u64,
    pub round: u64,
    pub white: u64,
    pub black: u64,
    pub result: u64,
}

impl HeaderCoverage {
    pub fn add(&mut self, other: Self) {
        self.event += other.event;
        self.site += other.site;
        self.date += other.date;
        self.round += other.round;
        self.white += other.white;
        self.black += other.black;
        self.result += other.result;
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct DateStats {
    pub complete: u64,
    pub incomplete_or_invalid: u64,
    pub missing: u64,
    pub earliest: Option<String>,
    pub latest: Option<String>,
}

impl DateStats {
    pub fn add(&mut self, other: &Self) {
        self.complete += other.complete;
        self.incomplete_or_invalid += other.incomplete_or_invalid;
        self.missing += other.missing;
        if let Some(value) = &other.earliest {
            if self.earliest.as_ref().is_none_or(|current| value < current) {
                self.earliest = Some(value.clone());
            }
        }
        if let Some(value) = &other.latest {
            if self.latest.as_ref().is_none_or(|current| value > current) {
                self.latest = Some(value.clone());
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct RatingStats {
    pub numeric: u64,
    pub invalid: u64,
    pub missing: u64,
    pub minimum: Option<u32>,
    pub average: f64,
    pub maximum: Option<u32>,
    pub distribution: RatingDistribution,
    #[serde(skip)]
    total: u64,
}

impl RatingStats {
    pub fn add(&mut self, other: Self) {
        self.numeric += other.numeric;
        self.invalid += other.invalid;
        self.missing += other.missing;
        self.total += other.total;
        self.distribution.add(other.distribution);
        if let Some(value) = other.minimum {
            self.minimum = Some(self.minimum.map_or(value, |current| current.min(value)));
        }
        if let Some(value) = other.maximum {
            self.maximum = Some(self.maximum.map_or(value, |current| current.max(value)));
        }
        self.update_average();
    }

    fn record(&mut self, value: Option<u32>) {
        let Some(value) = value else {
            self.invalid += 1;
            return;
        };
        self.numeric += 1;
        self.total += u64::from(value);
        self.distribution.record(value);
        self.minimum = Some(self.minimum.map_or(value, |current| current.min(value)));
        self.maximum = Some(self.maximum.map_or(value, |current| current.max(value)));
    }

    fn update_average(&mut self) {
        #[allow(clippy::cast_precision_loss)]
        if self.numeric > 0 {
            self.average = self.total as f64 / self.numeric as f64;
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatsDiagnosticCategory {
    Syntax,
    Input,
    Limit,
}

impl StatsDiagnosticCategory {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Input => "input",
            Self::Limit => "limit",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct StatsDiagnostic {
    pub category: StatsDiagnosticCategory,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte: Option<u64>,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct StatsReport {
    pub schema_version: u8,
    pub status: StatsStatus,
    pub source: String,
    pub outcome_required: bool,
    pub bytes: u64,
    pub games: u64,
    pub mainline_plies: u64,
    pub results: ResultCounts,
    pub game_length: GameLengthStats,
    pub header_coverage: HeaderCoverage,
    pub dates: DateStats,
    pub ratings: RatingStats,
    pub time_controls: TimeControlStats,
    pub elapsed_seconds: f64,
    pub throughput_mib_per_second: f64,
    pub diagnostic: Option<StatsDiagnostic>,
}

impl StatsReport {
    pub fn input_error(source: String, options: StatsOptions, message: String) -> Self {
        Self {
            schema_version: 1,
            status: StatsStatus::Error,
            source,
            outcome_required: options.require_outcome,
            bytes: 0,
            games: 0,
            mainline_plies: 0,
            results: ResultCounts::default(),
            game_length: GameLengthStats::default(),
            header_coverage: HeaderCoverage::default(),
            dates: DateStats::default(),
            ratings: RatingStats::default(),
            time_controls: TimeControlStats::default(),
            elapsed_seconds: 0.0,
            throughput_mib_per_second: 0.0,
            diagnostic: Some(StatsDiagnostic {
                category: StatsDiagnosticCategory::Input,
                byte: None,
                message,
            }),
        }
    }

    pub const fn exit_code(&self) -> u8 {
        match self.status {
            StatsStatus::Valid => 0,
            StatsStatus::Invalid => 1,
            StatsStatus::Error => 3,
        }
    }
}

#[derive(Debug)]
struct Accumulator {
    games: u64,
    mainline_plies: u64,
    results: ResultCounts,
    minimum_plies: Option<u64>,
    maximum_plies: Option<u64>,
    game_length_distribution: GameLengthDistribution,
    current_plies: u64,
    current_outcome: Outcome,
    variation_depth: u32,
    header_coverage: HeaderCoverage,
    dates_complete: u64,
    dates_incomplete_or_invalid: u64,
    dates_missing: u64,
    earliest_date: Option<u32>,
    latest_date: Option<u32>,
    ratings: RatingStats,
    time_controls: TimeControlStats,
    current_metadata: CurrentMetadata,
}

impl Default for Accumulator {
    fn default() -> Self {
        Self {
            games: 0,
            mainline_plies: 0,
            results: ResultCounts::default(),
            minimum_plies: None,
            maximum_plies: None,
            game_length_distribution: GameLengthDistribution::default(),
            current_plies: 0,
            current_outcome: Outcome::Unknown,
            variation_depth: 0,
            header_coverage: HeaderCoverage::default(),
            dates_complete: 0,
            dates_incomplete_or_invalid: 0,
            dates_missing: 0,
            earliest_date: None,
            latest_date: None,
            ratings: RatingStats::default(),
            time_controls: TimeControlStats::default(),
            current_metadata: CurrentMetadata::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct HeaderPresence(u8);

impl HeaderPresence {
    const EVENT: u8 = 1 << 0;
    const SITE: u8 = 1 << 1;
    const DATE: u8 = 1 << 2;
    const ROUND: u8 = 1 << 3;
    const WHITE: u8 = 1 << 4;
    const BLACK: u8 = 1 << 5;
    const RESULT: u8 = 1 << 6;

    fn mark(&mut self, header: u8) {
        self.0 |= header;
    }

    const fn has(self, header: u8) -> bool {
        self.0 & header != 0
    }
}

#[derive(Clone, Copy, Debug, Default)]
enum MetadataValue {
    #[default]
    Missing,
    Invalid,
    Numeric(u32),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum TimeControlKind {
    SuddenDeath,
    Increment,
    MovesPerPeriod,
    MultiStage,
    Hourglass,
    Unknown,
    Unlimited,
    Invalid,
    #[default]
    Missing,
}

impl MetadataValue {
    fn parsed(value: Option<u32>) -> Self {
        value.map_or(Self::Invalid, Self::Numeric)
    }

    const fn is_missing(self) -> bool {
        matches!(self, Self::Missing)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct CurrentMetadata {
    headers: HeaderPresence,
    date: MetadataValue,
    utc_date: MetadataValue,
    white_elo: MetadataValue,
    black_elo: MetadataValue,
    time_control: TimeControlKind,
}

impl CurrentMetadata {
    fn observe_tag(&mut self, tag: Tag<'_>) {
        match tag.name() {
            b"Event" => self.headers.mark(HeaderPresence::EVENT),
            b"Site" => self.headers.mark(HeaderPresence::SITE),
            b"Date" => {
                self.headers.mark(HeaderPresence::DATE);
                if self.date.is_missing() {
                    self.date = MetadataValue::parsed(parse_complete_date(tag.raw_value()));
                }
            }
            b"UTCDate" => {
                if self.utc_date.is_missing() {
                    self.utc_date = MetadataValue::parsed(parse_complete_date(tag.raw_value()));
                }
            }
            b"Round" => self.headers.mark(HeaderPresence::ROUND),
            b"White" => self.headers.mark(HeaderPresence::WHITE),
            b"Black" => self.headers.mark(HeaderPresence::BLACK),
            b"Result" => self.headers.mark(HeaderPresence::RESULT),
            b"WhiteElo" => {
                if self.white_elo.is_missing() {
                    self.white_elo = MetadataValue::parsed(parse_unsigned(tag.raw_value()));
                }
            }
            b"BlackElo" => {
                if self.black_elo.is_missing() {
                    self.black_elo = MetadataValue::parsed(parse_unsigned(tag.raw_value()));
                }
            }
            b"TimeControl" => {
                if self.time_control == TimeControlKind::Missing {
                    self.time_control = classify_time_control(tag.raw_value());
                }
            }
            _ => {}
        }
    }
}

impl Accumulator {
    fn observe(&mut self, event: Event<'_>) {
        match event {
            Event::GameStart { .. } => {
                self.current_plies = 0;
                self.current_outcome = Outcome::Unknown;
                self.variation_depth = 0;
                self.current_metadata = CurrentMetadata::default();
            }
            Event::Tag(tag) => self.current_metadata.observe_tag(tag),
            Event::San(_) if self.variation_depth == 0 => self.current_plies += 1,
            Event::VariationStart(_) => self.variation_depth += 1,
            Event::VariationEnd(_) => self.variation_depth -= 1,
            Event::Outcome { outcome, .. } if self.variation_depth == 0 => {
                self.current_outcome = outcome;
            }
            Event::GameEnd { .. } => self.finish_game(),
            _ => {}
        }
    }

    fn finish_game(&mut self) {
        self.games += 1;
        self.mainline_plies += self.current_plies;
        self.results.record(self.current_outcome);
        self.minimum_plies = Some(self.minimum_plies.map_or(self.current_plies, |minimum| {
            minimum.min(self.current_plies)
        }));
        self.maximum_plies = Some(self.maximum_plies.map_or(self.current_plies, |maximum| {
            maximum.max(self.current_plies)
        }));
        self.game_length_distribution.record(self.current_plies);
        self.finish_metadata();
    }

    fn finish_metadata(&mut self) {
        let headers = self.current_metadata.headers;
        self.header_coverage.event += u64::from(headers.has(HeaderPresence::EVENT));
        self.header_coverage.site += u64::from(headers.has(HeaderPresence::SITE));
        self.header_coverage.date += u64::from(headers.has(HeaderPresence::DATE));
        self.header_coverage.round += u64::from(headers.has(HeaderPresence::ROUND));
        self.header_coverage.white += u64::from(headers.has(HeaderPresence::WHITE));
        self.header_coverage.black += u64::from(headers.has(HeaderPresence::BLACK));
        self.header_coverage.result += u64::from(headers.has(HeaderPresence::RESULT));

        let date = if self.current_metadata.date.is_missing() {
            self.current_metadata.utc_date
        } else {
            self.current_metadata.date
        };
        match date {
            MetadataValue::Numeric(date) => {
                self.dates_complete += 1;
                self.earliest_date =
                    Some(self.earliest_date.map_or(date, |current| current.min(date)));
                self.latest_date = Some(self.latest_date.map_or(date, |current| current.max(date)));
            }
            MetadataValue::Invalid => self.dates_incomplete_or_invalid += 1,
            MetadataValue::Missing => self.dates_missing += 1,
        }

        for rating in [
            self.current_metadata.white_elo,
            self.current_metadata.black_elo,
        ] {
            match rating {
                MetadataValue::Numeric(value) => self.ratings.record(Some(value)),
                MetadataValue::Invalid => self.ratings.record(None),
                MetadataValue::Missing => self.ratings.missing += 1,
            }
        }
        self.time_controls
            .record(self.current_metadata.time_control);
    }

    fn game_length(&self) -> GameLengthStats {
        #[allow(clippy::cast_precision_loss)]
        let average_plies = if self.games == 0 {
            0.0
        } else {
            self.mainline_plies as f64 / self.games as f64
        };
        GameLengthStats {
            minimum_plies: self.minimum_plies,
            average_plies,
            maximum_plies: self.maximum_plies,
            distribution: self.game_length_distribution,
        }
    }

    fn dates(&self) -> DateStats {
        DateStats {
            complete: self.dates_complete,
            incomplete_or_invalid: self.dates_incomplete_or_invalid,
            missing: self.dates_missing,
            earliest: self.earliest_date.map(format_date),
            latest: self.latest_date.map(format_date),
        }
    }

    fn ratings(&self) -> RatingStats {
        let mut ratings = self.ratings;
        ratings.update_average();
        ratings
    }
}

fn parse_complete_date(value: &[u8]) -> Option<u32> {
    if value.len() != 10 || value[4] != b'.' || value[7] != b'.' {
        return None;
    }
    let year = parse_unsigned(&value[..4])?;
    let month = parse_unsigned(&value[5..7])?;
    let day = parse_unsigned(&value[8..])?;
    if year == 0 || !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return None;
    }
    Some(year * 10_000 + month * 100 + day)
}

fn parse_unsigned(value: &[u8]) -> Option<u32> {
    if value.is_empty() {
        return None;
    }
    value.iter().try_fold(0_u32, |number, byte| {
        let digit = u32::from(byte.checked_sub(b'0')?);
        (digit <= 9)
            .then_some(number)
            .and_then(|number| number.checked_mul(10))?
            .checked_add(digit)
    })
}

fn classify_time_control(value: &[u8]) -> TimeControlKind {
    match value {
        b"?" => return TimeControlKind::Unknown,
        b"-" => return TimeControlKind::Unlimited,
        _ => {}
    }
    if value.contains(&b':') {
        let mut stages = value.split(|byte| *byte == b':');
        let first = stages.next();
        let second = stages.next();
        if first.is_some()
            && second.is_some()
            && first.into_iter().chain(second).chain(stages).all(|stage| {
                !matches!(
                    classify_single_time_control(stage),
                    TimeControlKind::Invalid
                )
            })
        {
            return TimeControlKind::MultiStage;
        }
        return TimeControlKind::Invalid;
    }
    classify_single_time_control(value)
}

fn classify_single_time_control(value: &[u8]) -> TimeControlKind {
    if let Some(seconds) = value.strip_prefix(b"*") {
        return if parse_unsigned(seconds).is_some_and(|seconds| seconds > 0) {
            TimeControlKind::Hourglass
        } else {
            TimeControlKind::Invalid
        };
    }
    if let Some((initial, increment)) = split_once(value, b'+') {
        return if parse_unsigned(initial).is_some() && parse_unsigned(increment).is_some() {
            TimeControlKind::Increment
        } else {
            TimeControlKind::Invalid
        };
    }
    if let Some((moves, seconds)) = split_once(value, b'/') {
        return if parse_unsigned(moves).is_some_and(|moves| moves > 0)
            && parse_unsigned(seconds).is_some_and(|seconds| seconds > 0)
        {
            TimeControlKind::MovesPerPeriod
        } else {
            TimeControlKind::Invalid
        };
    }
    if parse_unsigned(value).is_some_and(|seconds| seconds > 0) {
        TimeControlKind::SuddenDeath
    } else {
        TimeControlKind::Invalid
    }
}

fn split_once(value: &[u8], delimiter: u8) -> Option<(&[u8], &[u8])> {
    let index = value.iter().position(|byte| *byte == delimiter)?;
    Some((&value[..index], &value[index + 1..]))
}

const fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

const fn is_leap_year(year: u32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn format_date(date: u32) -> String {
    format!(
        "{:04}.{:02}.{:02}",
        date / 10_000,
        date / 100 % 100,
        date % 100
    )
}

pub fn inspect<R: Read>(reader: R, source: String, options: StatsOptions) -> StatsReport {
    let parser_options = IncrementalParserOptions {
        parser: if options.require_outcome {
            ParserOptions::STRICT
        } else {
            ParserOptions::LENIENT
        },
        ..IncrementalParserOptions::default()
    };
    let mut parser = IncrementalParser::with_options(reader, parser_options);
    let mut accumulator = Accumulator::default();
    let started = Instant::now();
    let result = parser.parse(|event| accumulator.observe(event));
    let elapsed_seconds = started.elapsed().as_secs_f64();
    let bytes = parser.stats().bytes_read;
    #[allow(clippy::cast_precision_loss)]
    let throughput_mib_per_second = if elapsed_seconds > 0.0 {
        bytes as f64 / (1024.0 * 1024.0) / elapsed_seconds
    } else {
        0.0
    };
    let (status, diagnostic) = match result {
        Ok(_) => (StatsStatus::Valid, None),
        Err(StreamParseError::Parse(error)) => (
            StatsStatus::Invalid,
            Some(StatsDiagnostic {
                category: StatsDiagnosticCategory::Syntax,
                byte: u64::try_from(error.offset).ok(),
                message: error.to_string(),
            }),
        ),
        Err(StreamParseError::Io(error)) => (
            StatsStatus::Error,
            Some(StatsDiagnostic {
                category: StatsDiagnosticCategory::Input,
                byte: None,
                message: error.to_string(),
            }),
        ),
        Err(StreamParseError::TokenTooLarge { offset, limit }) => (
            StatsStatus::Error,
            Some(StatsDiagnostic {
                category: StatsDiagnosticCategory::Limit,
                byte: Some(offset),
                message: format!(
                    "PGN token at byte {offset} exceeds the {limit}-byte streaming limit"
                ),
            }),
        ),
    };

    StatsReport {
        schema_version: 1,
        status,
        source,
        outcome_required: options.require_outcome,
        bytes,
        games: accumulator.games,
        mainline_plies: accumulator.mainline_plies,
        results: accumulator.results,
        game_length: accumulator.game_length(),
        header_coverage: accumulator.header_coverage,
        dates: accumulator.dates(),
        ratings: accumulator.ratings(),
        time_controls: accumulator.time_controls,
        elapsed_seconds,
        throughput_mib_per_second,
        diagnostic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_only_mainline_plies_and_outcomes() {
        let report = inspect(
            &b"1. e4 (1. d4 d5 1/2-1/2) e5 1-0\n\n1. d4 *\n"[..],
            String::from("memory"),
            StatsOptions {
                require_outcome: true,
            },
        );

        assert_eq!(report.status, StatsStatus::Valid);
        assert_eq!(report.games, 2);
        assert_eq!(report.mainline_plies, 3);
        assert_eq!(report.results.white_wins, 1);
        assert_eq!(report.results.draws, 0);
        assert_eq!(report.results.unfinished, 1);
        assert_eq!(report.game_length.minimum_plies, Some(1));
        assert!((report.game_length.average_plies - 1.5).abs() < f64::EPSILON);
        assert_eq!(report.game_length.maximum_plies, Some(2));
        assert_eq!(report.game_length.distribution.from_1_to_20, 2);
        assert_eq!(report.time_controls.missing, 2);
    }

    #[test]
    fn preserves_completed_counts_on_a_syntax_error() {
        let report = inspect(
            &b"1. e4 *\n\n1. d4"[..],
            String::from("memory"),
            StatsOptions {
                require_outcome: true,
            },
        );

        assert_eq!(report.status, StatsStatus::Invalid);
        assert_eq!(report.games, 1);
        assert_eq!(report.mainline_plies, 1);
        assert!(report.diagnostic.is_some());
        assert_eq!(report.dates.missing, 1);
        assert_eq!(report.ratings.missing, 2);
    }

    #[test]
    fn summarizes_metadata_only_for_complete_games() {
        let report = inspect(
            &br#"[Event "First"]
[Site "Local"]
[Date "2024.02.29"]
[Round "1"]
[White "Alpha"]
[Black "Beta"]
[Result "1-0"]
[WhiteElo "2400"]
[BlackElo "2300"]

1-0

[Event "Second"]
[UTCDate "2024.??.??"]
[WhiteElo "?"]
[BlackElo "2500"]

*

*
"#[..],
            String::from("memory"),
            StatsOptions {
                require_outcome: true,
            },
        );

        assert_eq!(report.games, 3);
        assert_eq!(report.header_coverage.event, 2);
        assert_eq!(report.header_coverage.site, 1);
        assert_eq!(report.header_coverage.date, 1);
        assert_eq!(report.header_coverage.result, 1);
        assert_eq!(report.dates.complete, 1);
        assert_eq!(report.dates.incomplete_or_invalid, 1);
        assert_eq!(report.dates.missing, 1);
        assert_eq!(report.dates.earliest.as_deref(), Some("2024.02.29"));
        assert_eq!(report.dates.latest.as_deref(), Some("2024.02.29"));
        assert_eq!(report.ratings.numeric, 3);
        assert_eq!(report.ratings.invalid, 1);
        assert_eq!(report.ratings.missing, 2);
        assert_eq!(report.ratings.minimum, Some(2300));
        assert!((report.ratings.average - 2400.0).abs() < f64::EPSILON);
        assert_eq!(report.ratings.maximum, Some(2500));
        assert_eq!(report.ratings.distribution.from_2200_to_2399, 1);
        assert_eq!(report.ratings.distribution.at_least_2400, 2);
    }

    #[test]
    fn validates_complete_calendar_dates() {
        assert_eq!(parse_complete_date(b"2000.02.29"), Some(20_000_229));
        assert_eq!(parse_complete_date(b"1900.02.29"), None);
        assert_eq!(parse_complete_date(b"2024.13.01"), None);
        assert_eq!(parse_complete_date(b"????.??.??"), None);
    }

    #[test]
    fn uses_utc_date_only_when_roster_date_is_absent() {
        let report = inspect(
            &b"[UTCDate \"2023.12.31\"]\n\n*\n\n[Date \"????.??.??\"]\n[UTCDate \"2024.01.01\"]\n\n*\n"[..],
            String::from("memory"),
            StatsOptions {
                require_outcome: true,
            },
        );

        assert_eq!(report.header_coverage.date, 1);
        assert_eq!(report.dates.complete, 1);
        assert_eq!(report.dates.incomplete_or_invalid, 1);
        assert_eq!(report.dates.earliest.as_deref(), Some("2023.12.31"));
        assert_eq!(report.dates.latest.as_deref(), Some("2023.12.31"));
    }

    #[test]
    fn assigns_distribution_boundaries_exactly_once() {
        let mut lengths = GameLengthDistribution::default();
        for value in [0, 1, 20, 21, 40, 41, 60, 61, 80, 81, 120, 121, 160, 161] {
            lengths.record(value);
        }
        assert_eq!(lengths.zero, 1);
        assert_eq!(lengths.from_1_to_20, 2);
        assert_eq!(lengths.from_21_to_40, 2);
        assert_eq!(lengths.from_41_to_60, 2);
        assert_eq!(lengths.from_61_to_80, 2);
        assert_eq!(lengths.from_81_to_120, 2);
        assert_eq!(lengths.from_121_to_160, 2);
        assert_eq!(lengths.at_least_161, 1);

        let mut ratings = RatingDistribution::default();
        for value in [999, 1000, 1199, 1200, 2399, 2400] {
            ratings.record(value);
        }
        assert_eq!(ratings.under_1000, 1);
        assert_eq!(ratings.from_1000_to_1199, 2);
        assert_eq!(ratings.from_1200_to_1399, 1);
        assert_eq!(ratings.from_2200_to_2399, 1);
        assert_eq!(ratings.at_least_2400, 1);
    }

    #[test]
    fn classifies_pgn_time_control_forms() {
        let cases: &[(&[u8], TimeControlKind)] = &[
            (b"300", TimeControlKind::SuddenDeath),
            (b"180+2", TimeControlKind::Increment),
            (b"40/7200", TimeControlKind::MovesPerPeriod),
            (b"40/7200:3600+30", TimeControlKind::MultiStage),
            (b"*60", TimeControlKind::Hourglass),
            (b"?", TimeControlKind::Unknown),
            (b"-", TimeControlKind::Unlimited),
            (b"", TimeControlKind::Invalid),
            (b"40/0", TimeControlKind::Invalid),
            (b"40/7200:", TimeControlKind::Invalid),
        ];
        for (value, expected) in cases {
            assert_eq!(classify_time_control(value), *expected, "{value:?}");
        }
    }
}
