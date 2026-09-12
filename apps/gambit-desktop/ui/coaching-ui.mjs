import { acceptSnapshot, sameQueue, scoreText, lossText, feedbackText, fenSquares, summaryText } from "./coaching-model.mjs";

const symbols = { P: "♙", N: "♘", B: "♗", R: "♖", Q: "♕", K: "♔", p: "♟", n: "♞", b: "♝", r: "♜", q: "♛", k: "♚" };
const names = { p: "pawn", n: "knight", b: "bishop", r: "rook", q: "queen", k: "king" };

export function coachingUI({ invoke, context, onDone, onLater, onUpdate = () => {}, onExerciseChange = () => {} }) {
  const el = (id) => document.getElementById(`coaching-${id}`);
  let snapshot = null;
  let busy = false;
  let selected = null;
  let boardKey = "";
  let activeGame = null;
  let forceHidden = false;
  let poll = null;
  let linePly = 0;
  let pendingMove = null;
  let gesture = null;

  function canMove() { return !busy && !snapshot?.practice_busy && !linePly && !pendingMove; }

  function clearMove() {
    pendingMove = null;
    selected = null;
    el("move").value = "";
  }

  function chooseSquare(point, name) {
    if (!canMove()) return;
    const options = current()?.move_options ?? [];
    const ownPiece = options.some(move => move.uci.startsWith(name));
    if (name === selected) selected = null;
    else if (!selected || ownPiece) {
      if (!ownPiece) { el("feedback").textContent = "Choose one of your pieces with a legal move."; return; }
      selected = name;
      el("feedback").textContent = `${name} selected. Click a highlighted square or drag the piece there.`;
    } else {
      const choices = options.filter(move => move.uci.startsWith(`${selected}${name}`));
      const move = choices.find(move => move.uci.length === 4 || move.uci.endsWith(el("promotion").value));
      if (!move) { el("feedback").textContent = "That destination is not legal. Choose a highlighted square."; return; }
      pendingMove = move;
      selected = null;
      el("move").value = move.uci;
      el("feedback").textContent = "Your move is on the board. Check move to evaluate it, or Undo move to choose another.";
    }
    render();
  }

  function receive(incoming) {
    snapshot = acceptSnapshot(snapshot, incoming, context()?.path);
    render();
    if (sameQueue(snapshot, context())) onUpdate(snapshot);
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

  async function start(analyze, recoverGameId = null) {
    const ctx = context();
    if (!ctx?.player || busy) return;
    busy = true;
    el("feedback").textContent = "Preparing local diagnosis…";
    render();
    try {
      receive(await invoke("start_coaching", { request: { expected_path: ctx.path, game_ids: ctx.gameIds,
        player: ctx.player, shared_ply: ctx.ply, analyze, recover_game_id: recoverGameId } }));
      receive(await invoke("coaching_status"));
      el("feedback").textContent = "";
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
      el("feedback").textContent = "Progress saved on this Mac.";
      if (kind === "attempt") {
        const attempt = current()?.record?.practice.attempts.at(-1);
        el("feedback").textContent = feedbackText(attempt?.verdict);
        if (attempt?.verdict === "strong") forceHidden = false;
        else clearMove();
      }
      if (kind === "reveal") { forceHidden = false; clearMove(); }
      if (kind === "replay") { forceHidden = true; linePly = 0; clearMove(); el("feedback").textContent = "Choose a move from the starting exercise position."; }
      if (kind === "done") onDone();
      if (kind === "later") onLater();
    } catch (error) { el("feedback").textContent = String(error); }
    finally { busy = false; render(); }
  }

  function renderBoard(point, fen = pendingMove?.fen ?? point.position_fen) {
    const options = current()?.move_options ?? [];
    const key = `${activeGame}:${fen}:${selected}:${linePly}:${options.length}:${busy}:${snapshot?.practice_busy}`;
    if (key === boardKey) return;
    boardKey = key;
    const target = el("board");
    const focus = target.contains(document.activeElement) ? document.activeElement.dataset.square : null;
    const squares = fenSquares(fen);
    if (point.position_fen.split(" ")[1] === "b") squares.reverse();
    target.replaceChildren();
    for (const [index, square] of squares.entries()) {
      const button = document.createElement("button");
      button.type = "button";
      button.dataset.square = square.name;
      button.className = `square ${(square.name.charCodeAt(0) + Number(square.name[1])) % 2 ? "light" : "dark"}`;
      button.classList.toggle("selected", square.name === selected);
      button.classList.toggle("legal-target", canMove() && Boolean(selected) && options.some(move => move.uci.startsWith(`${selected}${square.name}`)));
      button.classList.toggle("last-move", Boolean(pendingMove) && [pendingMove.uci.slice(0, 2), pendingMove.uci.slice(2, 4)].includes(square.name));
      button.classList.toggle("movable", canMove() && options.some(move => move.uci.startsWith(square.name)));
      button.tabIndex = square.name === (focus ?? selected ?? squares[0].name) ? 0 : -1;
      button.setAttribute("aria-label", `${square.name}, ${square.piece ? `${square.piece === square.piece.toUpperCase() ? "White" : "Black"} ${names[square.piece.toLowerCase()]}` : "empty"}`);
      button.setAttribute("aria-pressed", String(square.name === selected));
      button.textContent = symbols[square.piece] ?? "";
      button.draggable = false;
      button.setAttribute("aria-disabled", String(!canMove()));
      const coordinate = document.createElement("small");
      coordinate.textContent = square.name;
      button.append(coordinate);
      // Keyboard/assistive activation uses click. Pointer interaction is owned
      // by the stable board container; HTML drag is unreliable in WKWebView.
      button.addEventListener("click", (event) => {
        if (!event?.detail) chooseSquare(point, square.name);
      });
      button.addEventListener("pointerdown", (event) => {
        if (!canMove() || gesture || (event.button !== undefined && event.button !== 0)) return;
        event.preventDefault();
        gesture = { name: square.name, point, pointerId: event.pointerId, x: event.clientX, y: event.clientY,
          piece: options.some(move => move.uci.startsWith(square.name)) ? square.piece : null, dragged: false };
        target.setPointerCapture?.(event.pointerId);
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
    if (!ctx) { onExerciseChange(false); return; }
    if (snapshot?.path !== ctx.path) snapshot = null;
    const game = current();
    const exerciseKey = JSON.stringify([ctx.path, ctx.player?.toLowerCase(), ctx.ply, ctx.gameIds, ctx.gameId]);
    if (activeGame !== exerciseKey) {
      activeGame = exerciseKey;
      selected = null;
      forceHidden = false;
      linePly = 0;
      clearMove();
      gesture = null;
      el("drag-piece").hidden = true;
      el("line").open = false;
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
    el("recover").hidden = !game?.recoverable;
    el("recover").disabled = busy || Boolean(snapshot?.running || snapshot?.practice_busy);
    el("summary").textContent = matches ? summaryText(snapshot, ctx.deferredIds) : "Completed results are saved privately on this Mac.";
    const point = game?.record?.diagnosis.outcome.kind === "turning_point" ? game.record.diagnosis.outcome.evidence : null;
    onExerciseChange(Boolean(point) && !ctx.complete);
    el("panel").classList.toggle("has-exercise", Boolean(point));
    if (point && !running) el("progress").textContent = "Practice this turning point";
    el("analyze").hidden = Boolean(matches && snapshot.games.every(game => game.status === "ready"));
    const practice = game?.record?.practice;
    const solved = practice && practice.solution !== "unsolved";
    const shown = point && !forceHidden && (practice.revealed || solved);
    const positions = shown ? game.line_positions ?? [] : [];
    linePly = Math.min(linePly, Math.max(0, positions.length - 1));
    el("exercise").hidden = !point;
    el("answer").hidden = !shown;
    el("answer").textContent = shown ? `Stockfish preferred ${point.best_san}. Before: ${scoreText(point.before)}. After ${point.played_san}: ${scoreText(point.after)}. ${lossText(point.loss)}.` : "";
    el("line").hidden = !shown;
    el("pv").textContent = shown ? point.pv_san.slice(0, 6).join(" · ") : "";
    el("playback").hidden = positions.length < 2;
    el("line-start").disabled = linePly === 0;
    el("line-previous").disabled = linePly === 0;
    el("line-next").disabled = linePly >= positions.length - 1;
    el("line-status").textContent = shown && positions.length > 1
      ? linePly ? `Line move ${linePly} of ${positions.length - 1}: ${point.pv_san[linePly - 1]}. Return to Exercise position to enter a move.` : "Exercise position. Use Next line move to step through the continuation."
      : "";
    el("done").hidden = !game?.record;
    el("done").disabled = busy || snapshot?.practice_busy || Boolean(point && !practice.revealed && !solved);
    el("later").hidden = !point;
    el("later").disabled = busy || snapshot?.practice_busy;
    el("replay").hidden = !point || (!practice.revealed && !solved);
    for (const name of ["check", "reveal", "replay"]) el(name).disabled = busy || snapshot?.practice_busy;
    el("check").disabled ||= linePly > 0;
    el("move").disabled = busy || snapshot?.practice_busy || linePly > 0;
    const promoting = Boolean(pendingMove?.uci.length === 5 || (selected && game?.move_options?.some(move => move.uci.startsWith(selected) && move.uci.length === 5)));
    el("promotion").hidden = !promoting;
    el("promotion-label").hidden = !promoting;
    el("reset").disabled = busy || snapshot?.practice_busy || (!pendingMove && !selected && !linePly && !el("move").value);
    el("reset").textContent = linePly ? "Return to exercise" : "Undo move";
    el("move-status").textContent = linePly ? "Viewing the answer line — return to the exercise to move pieces."
      : pendingMove ? `Your move: ${pendingMove.uci.slice(0, 2)} → ${pendingMove.uci.slice(2, 4)}. Check it below.`
      : "Your turn. Drag a piece, or click a piece then a highlighted destination.";
    el("status").textContent = point ? `Move ${point.position_fen.split(" ")[5]} · ${point.position_fen.split(" ")[1] === "w" ? "White" : "Black"} to move. You played ${point.played_san}. Find a better move.`
      : game?.record ? `No clear turning point found at this analysis budget.${game.record.diagnosis.inconclusive_moves ? " Some decisions had inconclusive engine evidence." : ""}`
        : game?.message ?? (matches ? snapshot.message : null) ?? (game?.status === "analyzing" ? "This game is being analyzed. You can keep navigating." : "Analyze this set to find supported turning points after the shared opening position.");
    if (point) renderBoard(point, linePly ? positions[linePly] : pendingMove?.fen ?? point.position_fen);
    if (snapshot?.running || snapshot?.practice_busy) {
      if (!poll) poll = window.setTimeout(async () => { poll = null; try { receive(await invoke("coaching_status")); } catch { /* Next navigation retries. */ } }, 1000);
    }
  }

  function endGesture() {
    if (gesture) el("board").releasePointerCapture?.(gesture.pointerId);
    gesture = null;
    el("drag-piece").hidden = true;
    el("board").querySelectorAll("button").forEach(button => button.classList.toggle("drag-origin", false));
    el("board").querySelectorAll("button").forEach(button => {
      button.classList.toggle("selected", button.dataset.square === selected);
      button.classList.toggle("legal-target", Boolean(selected) && Boolean(current()?.move_options?.some(move => move.uci.startsWith(`${selected}${button.dataset.square}`))));
    });
  }
  el("board").addEventListener("pointermove", (event) => {
    if (!gesture?.piece || event.pointerId !== gesture.pointerId || Math.hypot(event.clientX - gesture.x, event.clientY - gesture.y) < 6) return;
    gesture.dragged = true;
    el("board").querySelectorAll("button").forEach(button => {
      button.classList.toggle("drag-origin", button.dataset.square === gesture.name);
      button.classList.toggle("selected", button.dataset.square === gesture.name);
      button.classList.toggle("legal-target", Boolean(current()?.move_options?.some(move => move.uci.startsWith(`${gesture.name}${button.dataset.square}`))));
    });
    const ghost = el("drag-piece");
    ghost.textContent = symbols[gesture.piece];
    ghost.style.left = `${event.clientX}px`;
    ghost.style.top = `${event.clientY}px`;
    ghost.hidden = false;
  });
  el("board").addEventListener("pointerup", (event) => {
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    const { name, point, dragged } = gesture;
    const destination = document.elementFromPoint(event.clientX, event.clientY)?.closest("[data-square]");
    endGesture();
    if (dragged) {
      if (!destination || !el("board").contains(destination) || destination.dataset.square === name) return;
      selected = name;
      chooseSquare(point, destination.dataset.square);
    } else chooseSquare(point, name);
  });
  el("board").addEventListener("pointercancel", endGesture);
  el("reset").addEventListener("click", () => {
    clearMove(); linePly = 0; el("feedback").textContent = "Choose another move from the exercise position."; render();
  });
  el("move").addEventListener("input", () => {
    pendingMove = current()?.move_options?.find(move => move.uci === el("move").value.trim().toLowerCase()) ?? null;
    selected = null; render();
  });
  el("promotion").addEventListener("change", () => {
    if (pendingMove?.uci.length !== 5) return;
    pendingMove = current()?.move_options?.find(move => move.uci === pendingMove.uci.slice(0, 4) + el("promotion").value) ?? pendingMove;
    el("move").value = pendingMove.uci; render();
  });

  el("analyze").addEventListener("click", () => start(true));
  el("recover").addEventListener("click", () => start(true, context()?.gameId));
  el("cancel").addEventListener("click", async () => {
    try { await invoke("cancel_coaching", { expectedPath: snapshot.path, generation: snapshot.generation }); receive(await invoke("coaching_status")); }
    catch (error) { el("feedback").textContent = String(error); }
  });
  el("form").addEventListener("submit", (event) => {
    event.preventDefault();
    if (linePly) return;
    const move = el("move").value.trim().toLowerCase();
    if (!/^[a-h][1-8][a-h][1-8][qrbn]?$/.test(move)) { el("feedback").textContent = "Enter a move such as e2e4, or a promotion such as e7e8q."; return; }
    void action("attempt", move);
  });
  for (const name of ["reveal", "done", "later", "replay"]) el(name).addEventListener("click", () => action(name));
  for (const [name, delta] of [["start", 0], ["previous", -1], ["next", 1]]) {
    el(`line-${name}`).addEventListener("click", () => {
      clearMove();
      linePly = delta ? Math.max(0, linePly + delta) : 0;
      selected = null;
      render();
    });
  }
  return { receive, load, render,
    defer: () => { if (!current()?.record) return false; void action("later"); return true; },
    summary: () => sameQueue(snapshot, context()) ? summaryText(snapshot, context()?.deferredIds) : null };
}
