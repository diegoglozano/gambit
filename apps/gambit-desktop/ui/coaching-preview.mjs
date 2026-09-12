// Synthetic browser-only demonstrations. Never used by the native invoke path.
let saved = null;
const point = {
  position_fen: "7k/5K2/6Q1/8/8/8/8/8 w - - 0 1", ply: 1,
  played_uci: "g6h7", played_san: "Qh7+", best_san: "Qg8#", best_uci: "g6g8", pv: ["g6g8"], pv_san: ["Qg8#"],
  before: { bound: "exact", score: { kind: "mate_for", value: 1 } },
  after: { bound: "exact", score: { kind: "centipawns", value: 0 } }, loss: { kind: "lost_forced_mate" },
};
function summary() {
  const ready = saved.games.filter((g) => g.record);
  const points = ready.filter((g) => g.record.diagnosis.outcome.kind === "turning_point");
  saved.summary = { games_analyzed: ready.length, turning_points: points.length,
    solved_without_reveal: points.filter((g) => g.record.practice.solution === "without_reveal").length,
    completed_after_reveal: points.filter((g) => g.record.practice.revealed && g.record.practice.solution !== "without_reveal" && g.record.practice.disposition === "completed").length,
    practice_again_later: points.filter((g) => g.record.practice.disposition === "again_later").length,
    no_clear_turning_point: ready.length - points.length, repeated_positions: [], move_range: points.length ? [1, 1] : null };
}
export async function mockCoaching(command, args) {
  if (command === "start_coaching") {
    const request = args.request;
    const records = saved?.path === request.expected_path ? saved.games : [];
    saved = { path: request.expected_path, player: request.player, shared_ply: request.shared_ply,
      generation: (saved?.generation ?? 0) + 1, revision: 0, running: request.analyze, practice_busy: false,
      games: request.game_ids.map((id) => records.find((g) => g.id === id) ?? { id, status: "unseen", record: null }) };
    if (request.analyze) {
      const generation = saved.generation;
      saved.games.forEach((game, index) => setTimeout(() => {
        if (saved.generation !== generation || saved.cancelled) return;
        game.status = "ready";
        game.record ??= { diagnosis: { inconclusive_moves: 0, outcome: index === saved.games.length - 1 ? { kind: "no_clear_turning_point" } : { kind: "turning_point", evidence: point } },
          practice: { revealed: false, solution: "unsolved", disposition: "active", attempts: [] } };
        saved.revision++;
        saved.running = index < saved.games.length - 1;
        summary();
      }, (index + 1) * 700));
    }
  }
  if (command === "cancel_coaching") { saved.cancelled = true; saved.running = false; saved.revision++; }
  if (command === "coaching_practice") {
    const practice = saved.games.find((g) => g.id === args.gameId).record.practice;
    const { kind, uci } = args.action;
    if (kind === "attempt") {
      const verdict = ["g6g8", "g6h6"].includes(uci) ? "strong" : uci === "g6h7" ? "try_again" : "illegal";
      practice.attempts.push({ uci, verdict });
      if (verdict === "strong" && practice.solution === "unsolved") practice.solution = practice.revealed ? "after_hint" : "without_reveal";
    }
    if (kind === "reveal") practice.revealed = true;
    if (kind === "done") practice.disposition = "completed";
    if (kind === "later") practice.disposition = "again_later";
    if (kind === "replay") practice.disposition = "active";
    saved.revision++;
  }
  if (saved) {
    for (const game of saved.games) {
      game.line_positions = game.record?.diagnosis.outcome.kind === "turning_point" && (game.record.practice.revealed || game.record.practice.solution !== "unsolved")
        ? [point.position_fen, "6Qk/5K2/8/8/8/8/8/8 b - - 1 1"] : [];
      game.move_options = game.record?.diagnosis.outcome.kind === "turning_point" ? [
        { uci: "g6g8", fen: "6Qk/5K2/8/8/8/8/8/8 b - - 1 1" },
        { uci: "g6h6", fen: "7k/5K2/7Q/8/8/8/8/8 b - - 1 1" },
        { uci: "g6h7", fen: "7k/5K1Q/8/8/8/8/8/8 b - - 1 1" },
      ] : [];
    }
    summary();
  }
  return structuredClone(saved);
}
