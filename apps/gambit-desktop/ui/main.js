const nativeInvoke = window.__TAURI__?.core?.invoke;

const pieces = {
  P: "♙", N: "♘", B: "♗", R: "♖", Q: "♕", K: "♔",
  p: "♟", n: "♞", b: "♝", r: "♜", q: "♛", k: "♚",
};

const state = {
  session: null,
  detail: null,
  ply: 0,
  player: null,
  managedUser: null,
};

const element = (id) => document.getElementById(id);
const invoke = nativeInvoke ?? mockInvoke;

element("sync-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const username = element("username").value.trim();
  const since = element("since").value.trim() || null;
  await withBusy("Building your library…", "Gambit is fetching and indexing your games locally.", async () => {
    const session = await invoke("sync_user", { input: { username, since } });
    await showSession(session);
    showToast(`${session.info.games.toLocaleString()} games are ready.`);
  });
});

element("open-database").addEventListener("click", openDatabase);
element("change-database").addEventListener("click", openDatabase);
element("import-pgn").addEventListener("click", importPgn);
element("import-pgn-workspace").addEventListener("click", importPgn);
element("player-filter").addEventListener("submit", async (event) => {
  event.preventDefault();
  state.player = element("player").value.trim() || null;
  await loadPage(0);
});
element("sync-again").addEventListener("click", async () => {
  if (!state.managedUser) return;
  await withBusy("Syncing your latest games…", "Only new or changed Lichess games will be indexed.", async () => {
    const session = await invoke("sync_user", { input: { username: state.managedUser, since: null } });
    await showSession(session);
    showToast("Your library is up to date.");
  });
});
element("previous-page").addEventListener("click", () => loadPage(Math.max(0, state.session.page.offset - state.session.page.limit)));
element("next-page").addEventListener("click", () => loadPage(state.session.page.offset + state.session.page.limit));
element("first-move").addEventListener("click", () => setPly(0));
element("previous-move").addEventListener("click", () => setPly(state.ply - 1));
element("next-move").addEventListener("click", () => setPly(state.ply + 1));
element("last-move").addEventListener("click", () => setPly(state.detail?.moves.length ?? 0));
element("lichess-link").addEventListener("click", async (event) => {
  event.preventDefault();
  const url = element("lichess-link").href;
  if (url) await invoke("open_game_url", { url });
});

window.addEventListener("keydown", (event) => {
  if (event.target instanceof HTMLInputElement) return;
  if (event.key === "ArrowLeft") setPly(state.ply - 1);
  if (event.key === "ArrowRight") setPly(state.ply + 1);
});

async function openDatabase() {
  await withBusy("Opening database…", "Reading your library locally.", async () => {
    const session = await invoke("choose_database");
    if (session) await showSession(session);
  });
}

async function importPgn() {
  await withBusy("Building your database…", "Choose a PGN file and where to save the new local library.", async () => {
    const session = await invoke("import_pgn");
    if (!session) return;
    await showSession(session);
    showToast(`${session.info.games.toLocaleString()} games imported.`);
  });
}

async function showSession(session) {
  const player = session.managed_user ?? null;
  state.session = session;
  state.player = player;
  state.managedUser = player;
  state.detail = null;
  state.ply = 0;
  element("welcome-screen").hidden = true;
  element("workspace").hidden = false;
  element("database-card").hidden = false;
  element("database-name").textContent = basename(session.path);
  element("database-path").textContent = session.path;
  element("player").value = player ?? "";
  element("sync-again").hidden = !player;
  element("library-title").textContent = player ? `${player}'s games` : "Your games";
  element("stat-games").textContent = session.info.games.toLocaleString();
  element("stat-positions").textContent = session.info.positions.toLocaleString();
  element("stat-dates").textContent = dateRange(session.info);
  renderPage(session.page);
  if (session.page.games.length) await selectGame(session.page.games[0].id);
}

async function loadPage(offset) {
  if (!state.session) return;
  try {
    const page = await invoke("list_games", { player: state.player, offset, limit: state.session.page.limit });
    state.session.page = page;
    renderPage(page);
    if (page.games.length) await selectGame(page.games[0].id);
  } catch (error) {
    showToast(String(error), true);
  }
}

function renderPage(page) {
  const list = element("game-list");
  list.replaceChildren();
  element("game-count").textContent = page.total === 1 ? "1 game" : `${page.total.toLocaleString()} games`;
  element("previous-page").disabled = page.offset === 0;
  element("next-page").disabled = page.offset + page.games.length >= page.total;
  if (!page.games.length) {
    const empty = document.createElement("p");
    empty.className = "empty-message";
    empty.textContent = "No games match this player.";
    list.append(empty);
    return;
  }
  for (const game of page.games) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "game-row";
    button.dataset.gameId = game.id;
    button.addEventListener("click", () => selectGame(game.id));
    button.append(
      row("game-row-top", text(game.date ?? "Unknown date"), text(resultLabel(game.result), "game-row-result")),
      playerRow(game.white, game.white_elo, "White"),
      playerRow(game.black, game.black_elo, "Black"),
    );
    list.append(button);
  }
}

async function selectGame(id) {
  try {
    const detail = await invoke("get_game", { id });
    state.detail = detail;
    state.ply = 0;
    document.querySelectorAll(".game-row").forEach((row) => row.classList.toggle("active", Number(row.dataset.gameId) === id));
    renderGame(detail);
  } catch (error) {
    showToast(String(error), true);
  }
}

function renderGame(detail) {
  const game = detail.summary;
  element("white-name").textContent = game.white ?? "White";
  element("black-name").textContent = game.black ?? "Black";
  element("white-rating").textContent = game.white_elo ? `· ${game.white_elo}` : "";
  element("black-rating").textContent = game.black_elo ? `· ${game.black_elo}` : "";
  element("game-result").textContent = resultLabel(game.result);
  element("game-date").textContent = game.date ?? game.event ?? "Game details";
  element("raw-pgn").textContent = detail.pgn;
  const link = element("lichess-link");
  if (game.site?.startsWith("https://lichess.org/")) {
    link.href = game.site;
    link.hidden = false;
  } else {
    link.hidden = true;
  }
  renderMoves(detail.moves);
  setPly(0);
}

function renderMoves(moves) {
  const list = element("move-list");
  list.replaceChildren();
  if (!moves.length) {
    const empty = document.createElement("p");
    empty.className = "empty-message";
    empty.textContent = "Board replay is unavailable for this game.";
    list.append(empty);
    return;
  }
  for (let index = 0; index < moves.length; index += 2) {
    const number = document.createElement("span");
    number.className = "move-number";
    number.textContent = `${Math.floor(index / 2) + 1}.`;
    list.append(number, moveButton(moves[index]));
    if (moves[index + 1]) list.append(moveButton(moves[index + 1]));
    else list.append(document.createElement("span"));
  }
}

function moveButton(move) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "move-button";
  button.dataset.ply = move.ply;
  button.textContent = move.san;
  button.addEventListener("click", () => setPly(move.ply));
  return button;
}

function setPly(requested) {
  if (!state.detail) return;
  const maximum = state.detail.moves.length;
  state.ply = Math.max(0, Math.min(requested, maximum));
  const board = state.ply === 0 ? state.detail.initial_board : state.detail.moves[state.ply - 1]?.board;
  const lastMove = state.ply === 0 ? null : state.detail.moves[state.ply - 1];
  renderBoard(board, lastMove);
  element("move-position").textContent = state.ply === 0 ? "Start" : `${state.ply} / ${maximum} · ${lastMove.san}`;
  document.querySelectorAll(".move-button").forEach((button) => button.classList.toggle("active", Number(button.dataset.ply) === state.ply));
  element("first-move").disabled = state.ply === 0;
  element("previous-move").disabled = state.ply === 0;
  element("next-move").disabled = state.ply === maximum;
  element("last-move").disabled = state.ply === maximum;
  document.querySelector(`.move-button[data-ply="${state.ply}"]`)?.scrollIntoView({ block: "nearest" });
}

function renderBoard(board, lastMove) {
  const target = element("board");
  target.replaceChildren();
  if (!board || board.length !== 64) {
    target.textContent = "Board unavailable";
    return;
  }
  for (let rank = 7; rank >= 0; rank -= 1) {
    for (let file = 0; file < 8; file += 1) {
      const squareName = `${String.fromCharCode(97 + file)}${rank + 1}`;
      const square = document.createElement("div");
      square.className = `square ${(file + rank) % 2 ? "light" : "dark"}`;
      if (lastMove && (lastMove.from === squareName || lastMove.to === squareName)) square.classList.add("last");
      const symbol = pieces[board[rank * 8 + file]];
      if (symbol) square.append(text(symbol, "piece"));
      if (file === 0) square.append(text(String(rank + 1), "coordinate rank"));
      if (rank === 0) square.append(text(String.fromCharCode(97 + file), "coordinate file"));
      target.append(square);
    }
  }
}

async function withBusy(title, copy, action) {
  const overlay = element("busy-overlay");
  element("busy-title").textContent = title;
  element("busy-copy").textContent = copy;
  overlay.hidden = false;
  try {
    await action();
  } catch (error) {
    showToast(String(error), true);
  } finally {
    overlay.hidden = true;
  }
}

function showToast(message, error = false) {
  const toast = element("toast");
  toast.textContent = message;
  toast.classList.toggle("error", error);
  toast.hidden = false;
  window.clearTimeout(showToast.timer);
  showToast.timer = window.setTimeout(() => { toast.hidden = true; }, 5000);
}

function playerRow(name, rating, fallback) {
  return row("game-row-player", text(name ?? fallback), text(rating ? String(rating) : "—"));
}

function row(className, ...children) {
  const node = document.createElement("div");
  node.className = className;
  node.append(...children);
  return node;
}

function text(value, className) {
  const node = document.createElement("span");
  if (className) node.className = className;
  node.textContent = value;
  return node;
}

function resultLabel(result) {
  return { white_win: "1–0", black_win: "0–1", draw: "½–½", unfinished: "*" }[result] ?? "—";
}

function dateRange(info) {
  if (!info.earliest_date || !info.latest_date) return "No dates";
  return info.earliest_date === info.latest_date ? formatDate(info.earliest_date) : `${formatDate(info.earliest_date)} – ${formatDate(info.latest_date)}`;
}

function formatDate(date) {
  const value = String(date);
  return `${value.slice(0, 4)}.${value.slice(4, 6)}.${value.slice(6, 8)}`;
}

function basename(path) {
  return path.split(/[\\/]/).pop() || path;
}

async function restorePreviousSession() {
  try {
    const session = await invoke("restore_session");
    if (session) await showSession(session);
  } catch (error) {
    showToast(`Your previous library could not be reopened: ${error}`, true);
  }
}

async function mockInvoke(command) {
  await new Promise((resolve) => setTimeout(resolve, command === "sync_user" ? 650 : 80));
  if (command === "get_game") return mockDetail();
  if (command === "list_games") return mockSession().page;
  return mockSession();
}

function mockSession() {
  const games = [
    { id: 1, source: "lichess", source_game: 1, event: "Rated rapid game", site: "https://lichess.org/abcdefgh", date: "2026.09.04", white: "diegoglozano", black: "QuietKnight", white_elo: 1241, black_elo: 1218, result: "white_win", mainline_plies: 3 },
    { id: 2, source: "lichess", source_game: 2, event: "Rated blitz game", site: "https://lichess.org/hgfedcba", date: "2026.09.03", white: "CastleCoffee", black: "diegoglozano", white_elo: 1188, black_elo: 1229, result: "black_win", mainline_plies: 42 },
    { id: 3, source: "lichess", source_game: 3, event: "Rated rapid game", site: "https://lichess.org/a1b2c3d4", date: "2026.09.02", white: "EndgameEnjoyer", black: "diegoglozano", white_elo: 1277, black_elo: 1234, result: "draw", mainline_plies: 67 },
  ];
  return {
    path: "/Users/diego/Library/Application Support/Gambit/collections/diegoglozano/diegoglozano.gambit",
    managed_user: "diegoglozano",
    info: { games: 1729, positions: 110859, earliest_date: 20250626, latest_date: 20260904 },
    page: { total: 1729, offset: 0, limit: 100, games },
  };
}

function mockDetail() {
  const initial = "RNBQKBNRPPPPPPPP................................pppppppprnbqkbnr";
  const e4 = movePiece(initial, 12, 28);
  const e5 = movePiece(e4, 52, 36);
  const nf3 = movePiece(e5, 6, 21);
  return {
    summary: mockSession().page.games[0],
    pgn: `[Event "Rated rapid game"]\n[Site "https://lichess.org/abcdefgh"]\n[White "diegoglozano"]\n[Black "QuietKnight"]\n[Result "1-0"]\n\n1. e4 e5 2. Nf3 1-0`,
    initial_board: initial,
    moves: [
      { ply: 1, san: "e4", from: "e2", to: "e4", board: e4 },
      { ply: 2, san: "e5", from: "e7", to: "e5", board: e5 },
      { ply: 3, san: "Nf3", from: "g1", to: "f3", board: nf3 },
    ],
  };
}

function movePiece(board, from, to) {
  const squares = [...board];
  squares[to] = squares[from];
  squares[from] = ".";
  return squares.join("");
}

renderBoard("RNBQKBNRPPPPPPPP................................pppppppprnbqkbnr", null);
if (nativeInvoke) restorePreviousSession();
