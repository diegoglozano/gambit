export function boardCoordinates(flipped) {
  const ranks = flipped ? [0, 1, 2, 3, 4, 5, 6, 7] : [7, 6, 5, 4, 3, 2, 1, 0];
  const files = flipped ? [7, 6, 5, 4, 3, 2, 1, 0] : [0, 1, 2, 3, 4, 5, 6, 7];
  return ranks.flatMap((rank, row) => files.map((file, column) => ({ rank, file, row, column })));
}

export function perspectivePlayerIsBlack(player, managedUser, black) {
  const perspectivePlayer = player ?? managedUser;
  return Boolean(
    perspectivePlayer
      && black
      && perspectivePlayer.toLowerCase() === black.toLowerCase(),
  );
}

export function containedScrollDelta(container, item, padding = 8) {
  if (item.top < container.top) return item.top - container.top - padding;
  if (item.bottom > container.bottom) return item.bottom - container.bottom + padding;
  return 0;
}

export function parseSyncDate(value) {
  const match = /^(\d{4})[.-](\d{2})[.-](\d{2})$/.exec(value ?? "");
  if (!match) return null;
  const timestamp = Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
  return Number.isNaN(timestamp) ? null : timestamp;
}

export function timelineProgress(start, end, reached) {
  if (start === null || reached === null || end <= start) return null;
  const percentage = ((reached - start) / (end - start)) * 100;
  return Math.max(1, Math.min(98, percentage));
}

export function createRequestGate() {
  let current = 0;
  return {
    next() {
      current += 1;
      return current;
    },
    isCurrent(request) {
      return request === current;
    },
    invalidate() {
      current += 1;
    },
  };
}

export function validateLiveFilters(filters) {
  if (!filters.player && (
    filters.opponent
    || filters.color
    || filters.minimum_rating
    || filters.maximum_rating
    || filters.result === "win"
    || filters.result === "loss"
  )) {
    return "Choose a player before using opponent, color, rating, win, or loss.";
  }
  if (filters.position && filters.position.trim().split(/\s+/).length !== 6) {
    return "Finish the six-field FEN to update the results.";
  }
  if (filters.since && filters.until && filters.since > filters.until) {
    return "The start date must not be later than the end date.";
  }
  if (
    filters.minimum_rating
    && filters.maximum_rating
    && Number(filters.minimum_rating) > Number(filters.maximum_rating)
  ) {
    return "The minimum rating must not exceed the maximum rating.";
  }
  return null;
}

export function formatExploreMonth(value) {
  const month = Number(value) % 100;
  const year = Math.floor(Number(value) / 100) % 100;
  const names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  return month >= 1 && month <= 12 ? `${names[month - 1]} ’${String(year).padStart(2, "0")}` : String(value);
}

export function selectFocusOpening(openings, minimumCompletedGames = 4) {
  const candidates = openings
    .map((opening) => {
      const wins = Number(opening.wins ?? 0);
      const draws = Number(opening.draws ?? 0);
      const losses = Number(opening.losses ?? 0);
      const completed = wins + draws + losses;
      return {
        ...opening,
        wins,
        draws,
        losses,
        completed,
        score: completed ? Math.round(((wins + draws / 2) / completed) * 100) : 0,
      };
    })
    .filter((opening) => opening.completed >= minimumCompletedGames && opening.losses > 0);

  candidates.sort((left, right) => {
    const leftPoints = left.wins * 2 + left.draws;
    const rightPoints = right.wins * 2 + right.draws;
    const scoreOrder = leftPoints * right.completed - rightPoints * left.completed;
    if (scoreOrder) return scoreOrder;
    if (left.losses !== right.losses) return right.losses - left.losses;
    if (left.completed !== right.completed) return right.completed - left.completed;
    return String(left.line ?? "").localeCompare(String(right.line ?? ""));
  });
  return candidates[0] ?? null;
}

export function formatPlayerRecord(results) {
  const wins = Number(results?.wins ?? 0);
  const draws = Number(results?.draws ?? 0);
  const losses = Number(results?.losses ?? 0);
  return wins + draws + losses ? `${wins.toLocaleString()}W · ${draws.toLocaleString()}D · ${losses.toLocaleString()}L` : null;
}

export function reviewSummaries(gameIds, details) {
  const byId = new Map(details.map((detail) => [detail.summary.id, detail.summary]));
  return gameIds.map((id) => byId.get(id)).filter(Boolean);
}
