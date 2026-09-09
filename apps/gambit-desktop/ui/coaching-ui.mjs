import { acceptSnapshot, sameQueue, scoreText, lossText, feedbackText, fenSquares, summaryText } from "./coaching-model.mjs";

const symbols = { P: "♙", N: "♘", B: "♗", R: "♖", Q: "♕", K: "♔", p: "♟", n: "♞", b: "♝", r: "♜", q: "♛", k: "♚" };
const names = { p: "pawn", n: "knight", b: "bishop", r: "rook", q: "queen", k: "king" };

export function coachingUI({ invoke, context, onDone, onLater }) {
  const el = (id) => document.getElementById(`coaching-${id}`);
  let snapshot = null;
  let busy = false;
  let selected = null;
  let boardKey = "";
  let activeGame = null;
  let forceHidden = false;
  let poll = null;

  function receive(incoming) {
    snapshot = acceptSnapshot(snapshot, incoming, context()?.path);
    render();
  }

  function current() {
    const ctx = context();
    return sameQueue(snapshot, ctx) ? snapshot.games.find((game) => game.id === ctx.gameId) : null;
  }

  async function load() {
    const ctx = context();
    if (!ctx) return render();
    if (snapshot?.path !== ctx.path) snapshot = null;
    try {
      receive(await invoke("coaching_status"));
      if (!sameQueue(snapshot, ctx) && !snapshot?.running && !snapshot?.practice_busy) await start(false);
    } catch (error) { el("feedback").textContent = String(error); }
    render();
  }

  async function start(analyze) {
    const ctx = context();
    if (!ctx?.player || busy) return;
    busy = true;
    el("feedback").textContent = "Preparing local diagnosis…";
    render();
    try {
      receive(await invoke("start_coaching", { request: { expected_path: ctx.path, game_ids: ctx.gameIds,
        player: ctx.player, shared_ply: ctx.ply, analyze } }));
      receive(await invoke("coaching_status"));
    } catch (error) { el("feedback").textContent = String(error); }
    finally { busy = false; render(); }
  }

  async function action(kind, uci) {
    const ctx = context();
    if (!sameQueue(snapshot, ctx) || busy || snapshot.practice_busy) return;
    const id = ctx.gameId;
    busy = true;
    el("feedback").textContent = kind === "attempt" ? "Checking your move locally…" : "Saving practice progress…";
    render();
    try {
      const result = await invoke("coaching_practice", { expectedPath: ctx.path, generation: snapshot.generation,
        gameId: id, action: { kind, ...(uci ? { uci } : {}) } });
      receive(result);
      if (!sameQueue(result, context()) || context()?.gameId !== id) return;
      if (kind === "attempt") {
        const attempt = current()?.record?.practice.attempts.at(-1);
        el("feedback").textContent = feedbackText(attempt?.verdict);
        if (attempt?.verdict === "strong") forceHidden = false;
      }
      if (kind === "reveal") forceHidden = false;
      if (kind === "replay") { forceHidden = true; el("feedback").textContent = "Choose a move from the starting exercise position."; }
      if (kind === "done") onDone();
      if (kind === "later") onLater();
    } catch (error) { el("feedback").textContent = String(error); }
    finally { busy = false; render(); }
  }

  function renderBoard(point) {
    const key = `${activeGame}:${point.position_fen}:${selected}`;
    if (key === boardKey) return;
    boardKey = key;
    const target = el("board");
    const focus = target.contains(document.activeElement) ? document.activeElement.dataset.square : null;
    const squares = fenSquares(point.position_fen);
    if (point.position_fen.split(" ")[1] === "b") squares.reverse();
    target.replaceChildren();
    for (const [index, square] of squares.entries()) {
      const button = document.createElement("button");
      button.type = "button";
      button.dataset.square = square.name;
      button.className = `square ${(square.name.charCodeAt(0) + Number(square.name[1])) % 2 ? "light" : "dark"}`;
      button.classList.toggle("selected", square.name === selected);
      button.tabIndex = square.name === (focus ?? selected ?? squares[0].name) ? 0 : -1;
      button.setAttribute("aria-label", `${square.name}, ${square.piece ? `${square.piece === square.piece.toUpperCase() ? "White" : "Black"} ${names[square.piece.toLowerCase()]}` : "empty"}`);
      button.setAttribute("aria-pressed", String(square.name === selected));
      button.textContent = symbols[square.piece] ?? "";
      const coordinate = document.createElement("small");
      coordinate.textContent = square.name;
      button.append(coordinate);
      button.addEventListener("click", () => {
        if (busy || snapshot?.practice_busy) return;
        if (!selected) { selected = square.name; el("feedback").textContent = `${selected} selected. Choose a destination.`; }
        else {
          const from = squares.find((s) => s.name === selected);
          const promotion = from?.piece.toLowerCase() === "p" && /[18]$/.test(square.name) ? el("promotion").value : "";
          el("move").value = `${selected}${square.name}${promotion}`;
          selected = null;
          el("feedback").textContent = "Move entered. Choose Check move to submit it.";
        }
        renderBoard(point);
      });
      button.addEventListener("keydown", (event) => {
        if (event.key === "Escape") { selected = null; renderBoard(point); return; }
        const delta = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -8, ArrowDown: 8 }[event.key];
        if (delta === undefined) return;
        event.preventDefault();
        event.stopPropagation();
        const next = Math.max(0, Math.min(63, index + delta));
        target.querySelectorAll("button").forEach((b, i) => { b.tabIndex = i === next ? 0 : -1; });
        target.children[next].focus();
      });
      target.append(button);
    }
    if (focus) target.querySelector(`[data-square="${focus}"]`)?.focus();
  }

  function render() {
    const ctx = context();
    el("panel").hidden = !ctx || ctx.complete;
    if (!ctx) return;
    if (snapshot?.path !== ctx.path) snapshot = null;
    const game = current();
    if (activeGame !== ctx.gameId) {
      activeGame = ctx.gameId;
      selected = null;
      forceHidden = false;
      el("move").value = "";
      el("feedback").textContent = "";
    }
    const matches = sameQueue(snapshot, ctx);
    const running = matches && snapshot.running;
    const active = matches ? snapshot.games.findIndex((g) => g.status === "analyzing") : -1;
    el("progress").textContent = running ? active >= 0 ? `Analyzing game ${active + 1} of ${ctx.gameIds.length}` : "Loading saved results…"
      : matches && snapshot.cancelled ? "Diagnosis paused. Completed games are saved." : "Local review diagnosis";
    el("analyze").disabled = busy || Boolean(snapshot?.running || snapshot?.practice_busy) || !ctx.player;
    el("analyze").textContent = matches && snapshot.games.some((g) => g.status === "ready") ? "Continue diagnosis" : "Analyze review set";
    el("cancel").hidden = !snapshot?.running && !snapshot?.practice_busy;
    el("cancel").disabled = false;
    el("summary").textContent = matches ? summaryText(snapshot) : "Completed results are saved privately on this Mac.";
    const point = game?.record?.diagnosis.outcome.kind === "turning_point" ? game.record.diagnosis.outcome.evidence : null;
    const practice = game?.record?.practice;
    const solved = practice && practice.solution !== "unsolved";
    const shown = point && !forceHidden && (practice.revealed || solved);
    el("exercise").hidden = !point;
    el("answer").hidden = !shown;
    el("answer").textContent = shown ? `Stockfish preferred ${point.best_san}. Before: ${scoreText(point.before)}. After ${point.played_san}: ${scoreText(point.after)}. ${lossText(point.loss)}.` : "";
    el("line").hidden = !shown;
    el("pv").textContent = shown ? point.pv_san.slice(0, 6).join(" · ") : "";
    el("done").hidden = !game?.record;
    el("done").disabled = busy || snapshot?.practice_busy || Boolean(point && !practice.revealed && !solved);
    el("later").hidden = !point;
    el("later").disabled = busy || snapshot?.practice_busy;
    el("replay").hidden = !point || (!practice.revealed && !solved);
    for (const name of ["check", "reveal", "replay"]) el(name).disabled = busy || snapshot?.practice_busy;
    el("move").disabled = busy || snapshot?.practice_busy;
    el("status").textContent = point ? `Move ${point.position_fen.split(" ")[5]} · ${point.position_fen.split(" ")[1] === "w" ? "White" : "Black"} to move. You played ${point.played_san}. Find a better move.`
      : game?.record ? `No clear turning point found at this analysis budget.${game.record.diagnosis.inconclusive_moves ? " Some decisions had inconclusive engine evidence." : ""}`
        : game?.message ?? (matches ? snapshot.message : null) ?? (game?.status === "analyzing" ? "This game is being analyzed. You can keep navigating." : "Analyze this set to find supported turning points after the shared opening position.");
    if (point) renderBoard(point);
    if (snapshot?.running || snapshot?.practice_busy) {
      if (!poll) poll = window.setTimeout(async () => { poll = null; try { receive(await invoke("coaching_status")); } catch { /* Next navigation retries. */ } }, 1000);
    }
  }

  el("analyze").addEventListener("click", () => start(true));
  el("cancel").addEventListener("click", async () => {
    try { await invoke("cancel_coaching", { expectedPath: snapshot.path, generation: snapshot.generation }); receive(await invoke("coaching_status")); }
    catch (error) { el("feedback").textContent = String(error); }
  });
  el("form").addEventListener("submit", (event) => {
    event.preventDefault();
    const move = el("move").value.trim().toLowerCase();
    if (!/^[a-h][1-8][a-h][1-8][qrbn]?$/.test(move)) { el("feedback").textContent = "Enter a move such as e2e4, or a promotion such as e7e8q."; return; }
    void action("attempt", move);
  });
  for (const name of ["reveal", "done", "later", "replay"]) el(name).addEventListener("click", () => action(name));
  return { receive, load, render, summary: () => sameQueue(snapshot, context()) ? summaryText(snapshot) : null };
}
