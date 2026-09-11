export function sameQueue(snapshot, context) {
  return Boolean(snapshot && context && snapshot.path === context.path
    && snapshot.player.trim().toLowerCase() === context.player?.trim().toLowerCase()
    && snapshot.shared_ply === context.ply
    && snapshot.games.length === context.gameIds.length
    && snapshot.games.every((game, index) => game.id === context.gameIds[index]));
}

export function acceptSnapshot(current, incoming, path) {
  if (!incoming || incoming.path !== path) return current;
  if (current?.path === path && (incoming.generation < current.generation
    || (incoming.generation === current.generation && incoming.revision < current.revision))) return current;
  return incoming;
}

export function recommendationLabel(snapshot, count) {
  const positions = snapshot?.games.filter(g => g.record?.diagnosis.outcome.kind === "turning_point").length ?? 0;
  if (positions) return `Practice ${positions} ${positions === 1 ? "position" : "positions"} →`;
  if (snapshot?.cancelled || snapshot?.running || snapshot?.games.some(g => g.status !== "unseen")) {
    return snapshot.games.every(g => g.status === "ready") ? "View diagnosis summary →" : "Continue diagnosis →";
  }
  return `Diagnose ${count} ${count === 1 ? "loss" : "losses"} →`;
}

// Cached practice is authoritative. Old 'reviewed' flags alone are not evidence
// that a position was diagnosed. Do not clear outcomes while cache loading runs.
export function reconcileCoachingProgress(progress, snapshot) {
  const reviewed = new Set(progress.reviewed_game_ids);
  const deferred = new Set(progress.deferred_game_ids);
  for (const game of snapshot.games) {
    const practice = game.record?.practice;
    if (practice) {
      reviewed.delete(game.id);
      deferred.delete(game.id);
      if (practice.disposition === "completed") reviewed.add(game.id);
      if (practice.disposition === "again_later") deferred.add(game.id);
    } else if (!snapshot.running) reviewed.delete(game.id);
  }
  return { ...progress, reviewed_game_ids: progress.game_ids.filter(id => reviewed.has(id)),
    deferred_game_ids: progress.game_ids.filter(id => deferred.has(id)) };
}

export function scoreText(evaluation) {
  const bound = { lower: "at least ", upper: "at most ", exact: "" }[evaluation.bound] ?? "";
  const score = evaluation.score;
  if (score.kind === "centipawns") return `${bound}${score.value >= 0 ? "+" : ""}${(score.value / 100).toFixed(2)} pawns`;
  return `${bound}mate ${score.kind === "mate_for" ? "for" : "against"} you${score.value ? ` in ${score.value}` : ""}`;
}

export function lossText(loss) {
  if (loss.kind === "centipawns") return `${loss.exact ? "" : "at least "}${(loss.minimum / 100).toFixed(2)} pawns of evaluation lost`;
  return loss.kind === "allowed_mate" ? "allowed a forced mate" : "lost a forced mate";
}

export function feedbackText(verdict) {
  return { strong: "Strong move — within the accepted tolerance.", try_again: "Try again — a stronger continuation is available.",
    inconclusive: "The engine evidence is inconclusive at this budget. Try another move or reveal the answer.",
    illegal: "That move is not legal." }[verdict] ?? "";
}

export function fenSquares(fen) {
  const rows = fen.split(" ")[0].split("/");
  if (rows.length !== 8) return [];
  return rows.flatMap((row, rankIndex) => {
    const pieces = [...row].flatMap((piece) => /[1-8]/.test(piece) ? Array(Number(piece)).fill("") : [piece]);
    if (pieces.length !== 8) return [];
    return pieces.map((piece, file) => ({ name: `${String.fromCharCode(97 + file)}${8 - rankIndex}`, piece }));
  });
}

export function summaryText(snapshot, deferredIds = []) {
  const summary = snapshot?.summary;
  if (!summary) return "Completed results are saved privately on this Mac.";
  const parts = [`${summary.games_analyzed} analyzed`, `${summary.turning_points} turning points`,
    `${summary.solved_without_reveal} solved without reveal`, `${summary.solved_after_hint ?? 0} solved after a hint`, `${summary.completed_after_reveal} completed after reveal`,
    `${new Set([...deferredIds, ...snapshot.games.filter(g => g.record?.practice.disposition === "again_later").map(g => g.id)]).size} for later`, `${summary.no_clear_turning_point} with no clear turning point`];
  const unsupported = snapshot.games.filter((g) => g.status === "unsupported").length;
  const failed = snapshot.games.filter((g) => g.status === "failed").length;
  const pending = snapshot.games.filter((g) => ["unseen", "analyzing"].includes(g.status)).length;
  parts.push(`${unsupported} unsupported`, `${failed} failed`, `${pending} pending`);
  if (summary.move_range) parts.push(`turning points on moves ${summary.move_range.join("–")}`);
  if (summary.centipawn_loss) {
    const loss = summary.centipawn_loss;
    parts.push(`mean loss ${loss.minimum_only ? "at least " : ""}${(loss.mean_cp / 100).toFixed(2)} pawns / median ${loss.minimum_only ? "at least " : ""}${(loss.median_cp / 100).toFixed(2)} pawns (centipawn results only)`);
  }
  for (const repeated of summary.repeated_positions) {
    parts.push(`an identical position in ${repeated.games} games`);
    for (const choice of repeated.repeated_choices) parts.push(`${choice.san} chosen in ${choice.games} of those games`);
  }
  return parts.join(" · ");
}
