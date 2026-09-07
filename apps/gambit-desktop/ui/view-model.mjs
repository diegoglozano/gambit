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

export function reviewPatternKey(opening, player = null) {
  return `v1:${String(player ?? "").trim().toLowerCase()}:${Number(opening?.ply ?? 0)}:${String(opening?.line ?? "")}`;
}

export function reviewProgressMatches(opening, progress, player = null) {
  return Boolean(progress && progress.pattern === reviewPatternKey(opening, player));
}

export function reconcileReviewProgress(opening, saved, player = null) {
  const gameIds = [...new Set((opening.review_game_ids?.length
    ? opening.review_game_ids
    : [opening.game_id]).map(Number).filter((id) => Number.isInteger(id) && id > 0))];
  const matches = reviewProgressMatches(opening, saved, player);
  const reviewed = matches
    ? [...new Set(saved.reviewed_game_ids ?? [])].filter((id) => gameIds.includes(id))
    : [];
  const deferred = matches
    ? [...new Set(saved.deferred_game_ids ?? [])].filter((id) => gameIds.includes(id) && !reviewed.includes(id))
    : [];
  const current = matches && gameIds.includes(saved.current_game_id)
    ? saved.current_game_id
    : gameIds.find((id) => !reviewed.includes(id) && !deferred.includes(id)) ?? gameIds[0] ?? null;
  return {
    pattern: reviewPatternKey(opening, player),
    title: opening.line || "Recurring opening losses",
    game_ids: gameIds,
    reviewed_game_ids: reviewed,
    deferred_game_ids: deferred,
    current_game_id: current,
    ply: Number(opening.ply ?? 0),
    matching_losses: Number(opening.losses ?? gameIds.length),
  };
}

export function prepareReviewProgress(opening, saved, player = null) {
  const progress = reconcileReviewProgress(opening, saved, player);
  if (progress.reviewed_game_ids.length === progress.game_ids.length) {
    progress.reviewed_game_ids = [];
    progress.deferred_game_ids = [];
  }
  const untouched = progress.game_ids.filter((id) => (
    !progress.reviewed_game_ids.includes(id) && !progress.deferred_game_ids.includes(id)
  ));
  if (!untouched.length && progress.deferred_game_ids.length) progress.deferred_game_ids = [];
  const available = progress.game_ids.filter((id) => (
    !progress.reviewed_game_ids.includes(id) && !progress.deferred_game_ids.includes(id)
  ));
  if (!available.includes(progress.current_game_id)) progress.current_game_id = available[0] ?? progress.game_ids[0] ?? null;
  return progress;
}

export function nextReviewGameId(progress, currentGameId) {
  const available = progress.game_ids.filter((id) => (
    !progress.reviewed_game_ids.includes(id) && !progress.deferred_game_ids.includes(id)
  ));
  if (!available.length) return null;
  const currentIndex = progress.game_ids.indexOf(currentGameId);
  return [...progress.game_ids.slice(currentIndex + 1), ...progress.game_ids.slice(0, currentIndex + 1)]
    .find((id) => available.includes(id)) ?? available[0];
}
