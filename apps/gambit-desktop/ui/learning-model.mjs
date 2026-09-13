import { acceptSnapshot, sameQueue, fenSquares } from "./coaching-model.mjs";

export function lessonFinding(snapshot, summaries = []) {
  const points = snapshot?.games.filter(game => game.record?.diagnosis.outcome.kind === "turning_point") ?? [];
  const identity = game => {
    const point = game.record.diagnosis.outcome.evidence;
    return `${point.position_fen.split(" ").slice(0, 4).join(" ")}:${point.played_uci}`;
  };
  const rank = game => {
    const point = game.record.diagnosis.outcome.evidence;
    const count = points.filter(other => identity(other) === identity(game)).length;
    const consequence = point.loss.kind === "centipawns" ? Math.min(point.loss.minimum, 1000) / 100 : 12;
    const confidence = point.loss.kind !== "centipawns" || point.loss.exact ? 1 : 0;
    const recency = summaries.findIndex(summary => summary.id === game.id);
    return count * 4 + consequence + confidence - Math.max(0, recency) * 0.25;
  };
  const active = points.filter(game => game.record.practice.disposition === "active" && game.record.practice.solution === "unsolved" && !game.record.practice.revealed);
  const later = points.filter(game => game.record.practice.disposition === "again_later");
  const candidates = active.length ? active : later.length ? later : points.filter(game => game.record.practice.disposition !== "skipped");
  const game = candidates.slice().sort((a, b) => rank(b) - rank(a))[0];
  if (!game) return null;
  const point = game.record.diagnosis.outcome.evidence;
  return { game, point, source: summaries.find(summary => summary.id === game.id),
    supportingIds: points.filter(other => identity(other) === identity(game)).map(other => other.id),
    revisit: !active.length && !later.length };
}

export function lessonBoard(fen) { return Array.from({ length: 64 }, (_, index) => {
  const file = index % 8, rank = Math.floor(index / 8) + 1;
  return fenSquares(fen).find(square => square.name === `${String.fromCharCode(97 + file)}${rank}`)?.piece ?? "";
}); }

// Own preparation independently of the current game/lesson interaction. Never
// replace a manually opened queue or resume interrupted work silently.
export function learningFlow({ invoke, onUpdate, blocked = () => false, setTimer = setTimeout, clearTimer = clearTimeout }) {
  let plan = null;
  let token = 0;
  let timer = null;
  const notify = () => onUpdate(plan);
  const valid = version => version === token && plan && !blocked();
  function context() { return plan ? { path: plan.path, player: plan.player, ply: 0, gameIds: plan.games.map(game => game.id) } : null; }
  function receive(snapshot) {
    if (!sameQueue(snapshot, context())) {
      if (plan?.waiting && snapshot && !snapshot.running && !snapshot.practice_busy && !blocked() && !plan.loading) {
        plan.waiting = false;
        void prepare({ path: plan.path, player: plan.player }, { refresh: true });
      }
      return;
    }
    plan.snapshot = acceptSnapshot(plan.snapshot, snapshot, plan.path);
    plan.paused ||= Boolean(plan.snapshot.cancelled);
    notify();
    schedule();
  }
  function schedule() {
    if (timer || !plan?.snapshot?.running && !plan?.snapshot?.practice_busy) return;
    const version = token;
    timer = setTimer(async () => {
      timer = null;
      if (version !== token) return;
      try { receive(await invoke("coaching_status")); }
      catch { if (plan) { plan.error = "Could not check lesson preparation. Resume to try again."; notify(); } }
    }, 1000);
  }
  function reset() {
    token++;
    if (timer) clearTimer(timer);
    timer = null;
    plan = null;
  }
  async function prepare({ path, player }, { refresh = false, resume = false } = {}) {
    if (blocked()) return;
    if (plan?.path === path && plan.player === player && !refresh && !resume) return notify();
    if (plan?.loading) return;
    const version = ++token;
    if (!plan || plan.path !== path || plan.player !== player || refresh) plan = { path, player, games: [], snapshot: null, paused: false };
    plan.loading = true;
    plan.error = null;
    notify();
    try {
      if (!plan.games.length || refresh) plan.games = await invoke("lesson_games", { expectedPath: path, player });
      if (!valid(version) || !plan.games.length) return;
      const ctx = context();
      const active = await invoke("coaching_status");
      if (!valid(version)) return;
      if (sameQueue(active, ctx) && (active.running || !resume)) { receive(active); return; }
      if (active?.running || active?.practice_busy) {
        plan.waiting = true;
        plan.error = "Another lesson is using local analysis. Preparation will continue when it finishes.";
        return;
      }
      const request = { expected_path: path, game_ids: ctx.gameIds, player, shared_ply: 0, analyze: false };
      const cached = await invoke("coaching_overview", { request });
      if (!valid(version)) return;
      const paused = !resume && (plan.paused || cached.cancelled);
      const missing = cached.games.some(game => game.status === "unseen");
      plan.paused = paused;
      receive(await invoke("start_coaching", { request: { ...request, analyze: !paused && (resume || missing) } }));
      if (valid(version)) receive(await invoke("coaching_status"));
    } catch (error) {
      if (version === token && plan) plan.error = `Lesson preparation stopped: ${error}. Resume to try again, or browse your games.`;
    } finally {
      if (version === token && plan) { plan.loading = false; notify(); schedule(); }
    }
  }
  async function pause() {
    if (!plan?.snapshot) return;
    plan.paused = true;
    notify();
    await invoke("cancel_coaching", { expectedPath: plan.path, generation: plan.snapshot.generation });
    receive(await invoke("coaching_status"));
  }
  async function recover() {
    const game = plan?.snapshot?.games.find(game => game.recoverable);
    if (!game || blocked() || plan.snapshot.running || plan.snapshot.practice_busy) return;
    try {
      const ctx = context();
      plan.paused = false;
      plan.error = null;
      receive(await invoke("start_coaching", { request: { expected_path: ctx.path, player: ctx.player,
        game_ids: ctx.gameIds, shared_ply: 0, analyze: true, recover_game_id: game.id } }));
    } catch (error) { if (plan) { plan.error = String(error); notify(); } }
  }
  return { prepare, receive, pause, recover, reset, current: () => plan };
}
