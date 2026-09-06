import {
  boardCoordinates,
  containedScrollDelta,
  createRequestGate,
  formatExploreMonth,
  parseSyncDate,
  perspectivePlayerIsBlack,
  timelineProgress,
  validateLiveFilters,
} from "./view-model.mjs";

const nativeInvoke = window.__TAURI__?.core?.invoke;
const nativeListen = window.__TAURI__?.event?.listen;

const pieces = {
  P: "♙", N: "♘", B: "♗", R: "♖", Q: "♕", K: "♔",
  p: "♟", n: "♞", b: "♝", r: "♜", q: "♛", k: "♚",
};

const state = {
  session: null,
  detail: null,
  ply: 0,
  player: null,
  filters: {},
  managedUser: null,
  boardFlipped: false,
  syncTimelineStart: null,
  syncTimelineEnd: null,
  update: null,
  updateCheckRunning: false,
  sort: "date",
  sortDirection: "desc",
  explore: null,
  explorePlayer: null,
};

const element = (id) => document.getElementById(id);
const invoke = nativeInvoke ?? mockInvoke;
const pageRequests = createRequestGate();
const detailRequests = createRequestGate();
const exploreRequests = createRequestGate();
const FILTER_DEBOUNCE_MS = 250;
let filterTimer = null;

element("sync-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const username = element("username").value.trim();
  const since = element("since").value.trim() || null;
  const tokenInput = element("lichess-token");
  const token = tokenInput.value.trim() || null;
  try {
    await withBusy("Building your library…", "Lichess streams your game history before Gambit indexes it locally.", async () => {
      const session = await invoke("sync_user", { input: { username, since, token } });
      await showSession(session);
      showToast(`${session.info.games.toLocaleString()} games are ready.`);
    }, { sync: true, since });
  } finally {
    tokenInput.value = "";
  }
});

element("open-database").addEventListener("click", openDatabase);
element("change-database").addEventListener("click", showLibraries);
element("import-pgn").addEventListener("click", importPgn);
element("import-pgn-workspace").addEventListener("click", importPgn);
element("update-database").addEventListener("click", updateDatabase);
element("close-libraries").addEventListener("click", () => element("library-dialog").close());
element("open-another-database").addEventListener("click", async () => {
  element("library-dialog").close();
  await openDatabase();
});
element("check-updates").addEventListener("click", () => checkForUpdates(false));
element("dismiss-update").addEventListener("click", () => element("update-dialog").close());
element("install-update").addEventListener("click", installAvailableUpdate);
element("nav-library").addEventListener("click", () => showView("library"));
element("nav-explore").addEventListener("click", () => showView("explore"));
element("game-filters").addEventListener("submit", (event) => {
  event.preventDefault();
  queueLiveFilters(0);
});
element("game-filters").querySelectorAll("input, select").forEach((field) => {
  const eventName = field.tagName === "SELECT" || field.type === "date" ? "change" : "input";
  field.addEventListener(eventName, () => queueLiveFilters(eventName === "input" ? FILTER_DEBOUNCE_MS : 0));
});
element("clear-filters").addEventListener("click", () => {
  setFilterForm({});
  queueLiveFilters(0);
});
element("export-games").addEventListener("click", exportGames);
element("verify-database").addEventListener("click", verifyDatabase);
element("sort-games").addEventListener("change", () => {
  state.sort = element("sort-games").value;
  loadPage(0);
});
element("sort-direction").addEventListener("change", () => {
  state.sortDirection = element("sort-direction").value;
  loadPage(0);
});
element("sync-again").addEventListener("click", async () => {
  if (!state.managedUser) return;
  await withBusy("Syncing your latest games…", "Only new or changed Lichess games will be indexed.", async () => {
    const session = await invoke("sync_user", { input: { username: state.managedUser, since: null, token: null } });
    await showSession(session);
    showToast("Your library is up to date.");
  }, { sync: true });
});
element("previous-page").addEventListener("click", () => loadPage(Math.max(0, state.session.page.offset - state.session.page.limit)));
element("next-page").addEventListener("click", () => loadPage(state.session.page.offset + state.session.page.limit));
element("first-move").addEventListener("click", () => setPly(0));
element("previous-move").addEventListener("click", () => setPly(state.ply - 1));
element("next-move").addEventListener("click", () => setPly(state.ply + 1));
element("last-move").addEventListener("click", () => setPly(state.detail?.moves.length ?? 0));
element("flip-board").addEventListener("click", () => {
  state.boardFlipped = !state.boardFlipped;
  setPly(state.ply, false);
});
element("lichess-link").addEventListener("click", async (event) => {
  event.preventDefault();
  const url = element("lichess-link").href;
  if (url) await invoke("open_game_url", { url });
});

window.addEventListener("keydown", (event) => {
  if (!state.detail || event.metaKey || event.ctrlKey || event.altKey || document.querySelector("dialog[open]")) return;
  if (event.target instanceof Element && event.target.closest("input, textarea, select, [contenteditable='true']")) return;
  if (event.key === "ArrowLeft" || event.key === "ArrowRight") event.preventDefault();
  if (event.key === "ArrowLeft") setPly(state.ply - 1);
  if (event.key === "ArrowRight") setPly(state.ply + 1);
});

async function openDatabase() {
  await withBusy("Opening database…", "Reading your library locally.", async () => {
    const session = await invoke("choose_database");
    if (session) await showSession(session);
  });
}

async function showLibraries() {
  try {
    const libraries = await invoke("list_databases");
    renderLibraries(libraries);
    element("library-dialog").showModal();
  } catch (error) {
    showToast(String(error), true);
  }
}

function renderLibraries(libraries) {
  const list = element("library-list");
  list.replaceChildren();
  if (!libraries.length) {
    list.append(text("No recent databases yet.", "empty-message"));
    return;
  }
  for (const library of libraries) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "library-row";
    button.disabled = !library.exists;
    const title = text(basename(library.path));
    const detail = text(library.managed_user ? `Lichess · ${library.managed_user}` : library.path);
    button.append(title, detail);
    if (library.active) button.append(text("Active", "library-badge"));
    button.addEventListener("click", async () => {
      element("library-dialog").close();
      await withBusy("Opening database…", "Reading your library locally.", async () => {
        const session = await invoke("open_database", { path: library.path });
        await showSession(session);
      });
    });
    list.append(button);
  }
}

async function importPgn() {
  await withBusy("Building your database…", "Choose PGN files and where to save the new local library.", async () => {
    const session = await invoke("import_pgn");
    if (!session) return;
    await showSession(session);
    showToast(`${session.info.games.toLocaleString()} games imported.`);
  });
}

async function updateDatabase() {
  if (!state.session) return;
  await withBusy("Updating your database…", "Choose PGN files to add or refresh in this library.", async () => {
    const session = await invoke("update_database");
    if (!session) return;
    await showSession(session);
    showToast(`${session.info.games.toLocaleString()} games are ready.`);
  });
}

async function exportGames() {
  if (!state.session) return;
  await withBusy("Exporting games…", "Writing the matching games to a PGN file.", async () => {
    const report = await invoke("export_games", { filters: state.filters });
    if (report) showToast(`${report.games.toLocaleString()} matching games exported.`);
  });
}

async function verifyDatabase() {
  if (!state.session) return;
  await withBusy("Verifying database…", "Checking SQLite structure, stored PGN, and source fingerprints.", async () => {
    const info = await invoke("check_database");
    state.session.info = info;
    renderDatabaseInfo(info);
    if (info.integrity_issues.length) showToast(`${info.integrity_issues.length} integrity issue(s) found.`, true);
    else showToast("Database integrity verified.");
  });
}

async function checkForUpdates(silent) {
  if (state.updateCheckRunning) return;
  state.updateCheckRunning = true;
  const button = element("check-updates");
  button.disabled = true;
  button.textContent = "Checking…";
  try {
    const update = await invoke("check_for_update");
    if (!update) {
      if (!silent) showToast("Gambit is up to date.");
      return;
    }
    state.update = update;
    element("update-version").textContent = update.version;
    element("update-notes").textContent = update.notes?.trim() || "Download the update securely, install it, and restart Gambit.";
    if (!element("update-dialog").open) element("update-dialog").showModal();
  } catch (error) {
    if (!silent) showToast(`Could not check for updates: ${error}`, true);
  } finally {
    state.updateCheckRunning = false;
    button.disabled = false;
    button.textContent = "Check for updates";
  }
}

async function installAvailableUpdate() {
  if (!state.update) return;
  const version = state.update.version;
  element("update-dialog").close();
  await withBusy(`Installing Gambit ${version}…`, "The signed update is downloading. Gambit will restart when it is ready.", async () => {
    await invoke("install_update", { expectedVersion: version });
    await invoke("restart_app");
  });
}

async function showSession(session) {
  const player = session.managed_user ?? null;
  window.clearTimeout(filterTimer);
  filterTimer = null;
  pageRequests.invalidate();
  detailRequests.invalidate();
  exploreRequests.invalidate();
  state.session = session;
  state.player = player;
  state.filters = player ? { player } : {};
  state.managedUser = player;
  state.sort = "date";
  state.sortDirection = "desc";
  state.explore = null;
  state.explorePlayer = null;
  state.detail = null;
  state.ply = 0;
  element("welcome-screen").hidden = true;
  element("database-card").hidden = false;
  element("nav-explore").disabled = false;
  element("database-name").textContent = basename(session.path);
  element("database-path").textContent = session.path;
  setFilterForm(state.filters);
  element("sort-games").value = state.sort;
  element("sort-direction").value = state.sortDirection;
  setFilterStatus("Filters update automatically");
  element("sync-again").hidden = !player;
  element("library-title").textContent = player ? `${player}'s games` : "Your games";
  renderDatabaseInfo(session.info);
  renderPage(session.page);
  showView("library");
  if (session.page.games.length) await selectGame(session.page.games[0].id);
}

async function loadPage(offset, options = {}) {
  if (!state.session) return;
  const request = pageRequests.next();
  const selectedId = state.detail?.summary.id ?? null;
  setFilterStatus("Updating…", "pending");
  try {
    const page = await invoke("list_games", {
      filters: state.filters,
      sort: state.sort,
      direction: state.sortDirection,
      offset,
      limit: state.session.page.limit,
    });
    if (!pageRequests.isCurrent(request)) return;
    state.session.page = page;
    renderPage(page);
    setFilterStatus(`${gameCount(page.total)} matching`);
    if (selectedId !== null && page.games.some((game) => game.id === selectedId)) {
      markSelectedGame(selectedId);
      state.boardFlipped = perspectivePlayerIsBlack(state.player, state.managedUser, state.detail?.summary.black);
      setPly(state.ply, false);
    } else if (page.games.length) {
      await selectGame(page.games[0].id);
    } else {
      clearGame();
    }
  } catch (error) {
    if (!pageRequests.isCurrent(request)) return;
    setFilterStatus(String(error), "error");
    if (!options.live) showToast(String(error), true);
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
    empty.textContent = "No games match these filters.";
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
  const request = detailRequests.next();
  try {
    const detail = await invoke("get_game", { id });
    if (!detailRequests.isCurrent(request)) return;
    state.detail = detail;
    state.ply = 0;
    state.boardFlipped = perspectivePlayerIsBlack(state.player, state.managedUser, detail.summary.black);
    markSelectedGame(id);
    renderGame(detail);
  } catch (error) {
    if (detailRequests.isCurrent(request)) showToast(String(error), true);
  }
}

function markSelectedGame(id) {
  document.querySelectorAll(".game-row").forEach((row) => row.classList.toggle("active", Number(row.dataset.gameId) === id));
}

function clearGame() {
  detailRequests.invalidate();
  state.detail = null;
  state.ply = 0;
  element("white-name").textContent = "—";
  element("black-name").textContent = "No matching game";
  element("white-rating").textContent = "";
  element("black-rating").textContent = "";
  element("game-result").textContent = "—";
  element("game-date").textContent = "—";
  element("raw-pgn").textContent = "";
  element("lichess-link").hidden = true;
  element("move-list").replaceChildren(text("Adjust the filters to find a game.", "empty-message"));
  renderBoard(null, null);
  for (const id of ["first-move", "previous-move", "next-move", "last-move"]) element(id).disabled = true;
  element("move-position").textContent = "Start";
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
  setPly(0, false);
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

function setPly(requested, scrollMove = true) {
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
  if (scrollMove) scrollMoveIntoView(state.ply);
}

function renderBoard(board, lastMove) {
  const target = element("board");
  target.replaceChildren();
  if (!board || board.length !== 64) {
    target.textContent = "Board unavailable";
    return;
  }
  target.setAttribute("aria-label", `Chess position, ${state.boardFlipped ? "Black" : "White"} perspective`);
  for (const { rank, file, row, column } of boardCoordinates(state.boardFlipped)) {
    const squareName = `${String.fromCharCode(97 + file)}${rank + 1}`;
    const square = document.createElement("div");
    square.className = `square ${(file + rank) % 2 ? "light" : "dark"}`;
    if (lastMove && (lastMove.from === squareName || lastMove.to === squareName)) square.classList.add("last");
    const symbol = pieces[board[rank * 8 + file]];
    if (symbol) square.append(text(symbol, "piece"));
    if (column === 0) square.append(text(String(rank + 1), "coordinate rank"));
    if (row === 7) square.append(text(String.fromCharCode(97 + file), "coordinate file"));
    target.append(square);
  }
}

function scrollMoveIntoView(ply) {
  if (ply === 0) return;
  const list = element("move-list");
  const button = list.querySelector(`.move-button[data-ply="${ply}"]`);
  if (!button) return;
  const listBounds = list.getBoundingClientRect();
  const buttonBounds = button.getBoundingClientRect();
  list.scrollTop += containedScrollDelta(listBounds, buttonBounds);
}

function beginSyncProgress(since) {
  state.syncTimelineStart = parseSyncDate(since);
  state.syncTimelineEnd = Date.now();
  const progress = element("sync-progress-bar");
  progress.removeAttribute("value");
  element("sync-progress-copy").textContent = "Connecting to Lichess…";
  element("sync-progress").hidden = false;
}

function updateSyncProgress(progress) {
  if (element("sync-progress").hidden) return;
  const bar = element("sync-progress-bar");
  const copy = element("sync-progress-copy");
  if (progress.phase === "connecting") {
    bar.removeAttribute("value");
    copy.textContent = "Connecting to Lichess…";
    return;
  }
  if (progress.phase === "indexing") {
    bar.value = 100;
    copy.textContent = `Downloaded ${gameCount(progress.games)}. Building the local index…`;
    return;
  }
  const reached = parseSyncDate(progress.date);
  if (reached !== null && state.syncTimelineStart === null) state.syncTimelineStart = reached;
  const percentage = timelineProgress(state.syncTimelineStart, state.syncTimelineEnd, reached);
  if (percentage === null) bar.removeAttribute("value");
  else bar.value = percentage;
  const date = progress.date ? ` · reached ${progress.date.replaceAll(".", "-")}` : "";
  copy.textContent = `Downloaded ${gameCount(progress.games)}${date}`;
}

function gameCount(games) {
  return `${Number(games).toLocaleString()} ${games === 1 ? "game" : "games"}`;
}

async function withBusy(title, copy, action, options = {}) {
  const overlay = element("busy-overlay");
  element("busy-title").textContent = title;
  element("busy-copy").textContent = copy;
  if (options.sync) beginSyncProgress(options.since ?? null);
  else element("sync-progress").hidden = true;
  overlay.hidden = false;
  try {
    await action();
  } catch (error) {
    showToast(String(error), true);
  } finally {
    overlay.hidden = true;
    element("sync-progress").hidden = true;
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

function queueLiveFilters(delay) {
  window.clearTimeout(filterTimer);
  pageRequests.invalidate();
  setFilterStatus(delay ? "Waiting for you to finish typing…" : "Updating…", "pending");
  filterTimer = window.setTimeout(applyLiveFilters, delay);
}

async function applyLiveFilters() {
  filterTimer = null;
  const filters = readFilters();
  const validation = validateLiveFilters(filters);
  if (validation) {
    setFilterStatus(validation, "error");
    return;
  }
  const previousPlayer = state.player?.toLowerCase() ?? null;
  state.filters = filters;
  state.player = filters.player ?? null;
  if ((state.player?.toLowerCase() ?? null) !== previousPlayer) {
    state.explore = null;
    exploreRequests.invalidate();
  }
  await loadPage(0, { live: true });
}

function setFilterStatus(message, tone = "") {
  const status = element("filter-status");
  status.textContent = message;
  status.classList.toggle("pending", tone === "pending");
  status.classList.toggle("error", tone === "error");
  element("export-games").disabled = tone === "pending" || tone === "error";
}

function showView(view) {
  if (!state.session) return;
  const exploring = view === "explore";
  element("workspace").hidden = exploring;
  element("explore-view").hidden = !exploring;
  element("nav-library").classList.toggle("active", !exploring);
  element("nav-explore").classList.toggle("active", exploring);
  if (exploring) loadExplore();
}

async function loadExplore() {
  if (!state.session) return;
  const player = state.filters.player ?? state.managedUser ?? null;
  if (state.explore && state.explorePlayer?.toLowerCase() === player?.toLowerCase()) {
    renderExplore(state.explore);
    return;
  }
  const request = exploreRequests.next();
  element("explore-content").setAttribute("aria-busy", "true");
  element("explore-scope").textContent = "Reading patterns from your local database…";
  try {
    const report = await invoke("explore_database", { player });
    if (!exploreRequests.isCurrent(request)) return;
    state.explore = report;
    state.explorePlayer = player;
    renderExplore(report);
  } catch (error) {
    if (!exploreRequests.isCurrent(request)) return;
    element("explore-scope").textContent = `Explore could not be loaded: ${error}`;
    showToast(String(error), true);
  } finally {
    if (exploreRequests.isCurrent(request)) element("explore-content").removeAttribute("aria-busy");
  }
}

function renderExplore(report) {
  const focus = report.player;
  element("explore-scope").textContent = focus
    ? `Patterns from games featuring ${focus}. Change the Player filter in Library to explore someone else.`
    : "Patterns across every player in this local library.";
  element("explore-game-count").textContent = gameCount(report.games);
  element("players-title").textContent = focus ? "Frequent opponents" : "Most active players";
  element("timeline-legend").textContent = focus ? "Wins · Draws · Losses" : "White · Draws · Black";
  renderOpenings(report.openings, report.games);
  renderTimeline(report.timeline);
  renderExplorePlayers(report.players);
  renderCommonPositions(report.positions);
}

function renderOpenings(openings, total) {
  const list = element("opening-list");
  list.replaceChildren();
  if (!openings.length) {
    renderExploreEmpty(list, "No standard opening positions are available.");
    return;
  }
  openings.forEach((opening, index) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "ranked-row";
    const percentage = total ? Math.round((opening.games / total) * 100) : 0;
    button.append(
      text(String(index + 1).padStart(2, "0"), "ranked-number"),
      row("ranked-copy", text(opening.line || "Starting position", "ranked-title"), text(`${percentage}% of explored games`)),
      text(gameCount(opening.games), "ranked-value"),
    );
    button.addEventListener("click", () => openExplorePosition(opening));
    list.append(button);
  });
}

function renderTimeline(periods) {
  const chart = element("timeline-chart");
  chart.replaceChildren();
  if (!periods.length) {
    renderExploreEmpty(chart, "No dated games are available.");
    return;
  }
  const maximum = Math.max(...periods.map((period) => period.games), 1);
  for (const period of periods) {
    const timeline = document.createElement("div");
    timeline.className = "timeline-row";
    const stack = document.createElement("div");
    stack.className = "timeline-stack";
    stack.style.width = `${Math.max(8, (period.games / maximum) * 100)}%`;
    stack.title = `${period.wins} / ${period.draws} / ${period.losses}`;
    for (const [kind, count] of [["win", period.wins], ["draw", period.draws], ["loss", period.losses], ["unfinished", period.unfinished]]) {
      if (!count) continue;
      const segment = document.createElement("span");
      segment.className = `timeline-segment ${kind}`;
      segment.style.flexBasis = `${(count / period.games) * 100}%`;
      stack.append(segment);
    }
    timeline.append(text(formatExploreMonth(period.month)), stack, text(String(period.games), "timeline-total"));
    chart.append(timeline);
  }
}

function renderExplorePlayers(players) {
  const list = element("player-list");
  list.replaceChildren();
  if (!players.length) {
    renderExploreEmpty(list, "No named players are available.");
    return;
  }
  players.forEach((player, index) => {
    const item = row(
      "ranked-row",
      text(String(index + 1).padStart(2, "0"), "ranked-number"),
      row("ranked-copy", text(player.name, "ranked-title"), text(`${player.wins} · ${player.draws} · ${player.losses}`)),
      text(gameCount(player.games), "ranked-value"),
    );
    list.append(item);
  });
}

function renderCommonPositions(positions) {
  const list = element("position-list");
  list.replaceChildren();
  if (!positions.length) {
    renderExploreEmpty(list, "No repeated middlegame positions were found.");
    return;
  }
  for (const position of positions) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "position-card";
    button.append(
      renderMiniBoard(position.board),
      row(
        "position-copy",
        text(gameCount(position.games), "position-count"),
        text(`First seen after ${Math.ceil(position.ply / 2)} moves`),
        text(position.line || "Standard position"),
      ),
    );
    button.addEventListener("click", () => openExplorePosition(position));
    list.append(button);
  }
}

function renderMiniBoard(board) {
  const target = document.createElement("div");
  target.className = "mini-board";
  target.setAttribute("aria-hidden", "true");
  for (const { rank, file } of boardCoordinates(false)) {
    const square = document.createElement("span");
    square.className = `mini-square ${(file + rank) % 2 ? "light" : "dark"}`;
    square.textContent = pieces[board[rank * 8 + file]] ?? "";
    target.append(square);
  }
  return target;
}

function renderExploreEmpty(target, message) {
  target.append(text(message, "empty-message"));
}

async function openExplorePosition(position) {
  showView("library");
  await selectGame(position.game_id);
  setPly(position.ply);
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

function readFilters() {
  const values = {
    player: element("player").value,
    opponent: element("opponent").value,
    color: element("color").value,
    result: element("result").value,
    since: element("filter-since").value,
    until: element("filter-until").value,
    minimum_rating: element("minimum-rating").value,
    maximum_rating: element("maximum-rating").value,
    position: element("position").value,
  };
  return Object.fromEntries(
    Object.entries(values)
      .map(([key, value]) => [key, value.trim()])
      .filter(([, value]) => value),
  );
}

function setFilterForm(filters) {
  element("player").value = filters.player ?? "";
  element("opponent").value = filters.opponent ?? "";
  element("color").value = filters.color ?? "";
  element("result").value = filters.result ?? "";
  element("filter-since").value = filters.since ?? "";
  element("filter-until").value = filters.until ?? "";
  element("minimum-rating").value = filters.minimum_rating ?? "";
  element("maximum-rating").value = filters.maximum_rating ?? "";
  element("position").value = filters.position ?? "";
}

function renderDatabaseInfo(info) {
  element("stat-games").textContent = Number(info.games).toLocaleString();
  element("stat-positions").textContent = Number(info.positions).toLocaleString();
  const results = info.results;
  element("stat-results").textContent = results
    ? `${Number(results.white_wins).toLocaleString()} / ${Number(results.draws).toLocaleString()} / ${Number(results.black_wins).toLocaleString()}`
    : "—";
  element("stat-storage").textContent = humanBytes(info.pgn_bytes);
  element("stat-dates").textContent = dateRange(info);
  const integrity = element("stat-integrity");
  integrity.classList.remove("healthy", "unhealthy");
  if (!info.integrity_checked) {
    integrity.textContent = "Not checked";
  } else if (info.integrity_issues.length) {
    integrity.textContent = `${info.integrity_issues.length} issue(s)`;
    integrity.classList.add("unhealthy");
  } else {
    integrity.textContent = "Verified";
    integrity.classList.add("healthy");
  }
}

function humanBytes(bytes) {
  if (bytes === undefined || bytes === null) return "—";
  const value = Number(bytes);
  if (value < 1024) return `${value} B`;
  if (value < 1024 ** 2) return `${(value / 1024).toFixed(1)} KiB`;
  if (value < 1024 ** 3) return `${(value / 1024 ** 2).toFixed(1)} MiB`;
  return `${(value / 1024 ** 3).toFixed(1)} GiB`;
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

async function initializeNativeApp() {
  if (nativeListen) {
    try {
      await nativeListen("sync-progress", (event) => updateSyncProgress(event.payload));
    } catch {
      // Sync still has its indeterminate spinner if native progress events are unavailable.
    }
  }
  try {
    element("app-version").textContent = await invoke("app_version");
  } catch {
    element("app-version").textContent = "Desktop";
  }
  await restorePreviousSession();
  window.setTimeout(() => checkForUpdates(true), 1500);
}

async function mockInvoke(command, args = {}) {
  await new Promise((resolve) => setTimeout(resolve, command === "sync_user" ? 650 : 80));
  if (command === "app_version") return "Preview";
  if (command === "check_for_update") {
    return { current_version: "0.13.0", version: "0.14.0", notes: "A faster, friendlier Gambit is ready." };
  }
  if (command === "install_update" || command === "restart_app") return null;
  if (command === "get_game") return mockDetail();
  if (command === "list_games") {
    const page = mockSession().page;
    if (args.sort === "rating") page.games.sort((a, b) => Math.max(b.white_elo ?? 0, b.black_elo ?? 0) - Math.max(a.white_elo ?? 0, a.black_elo ?? 0));
    if (args.direction === "asc") page.games.reverse();
    return page;
  }
  if (command === "explore_database") return mockExplore(args.player);
  if (command === "list_databases") {
    const session = mockSession();
    return [{ path: session.path, managed_user: session.managed_user, exists: true, active: true }];
  }
  if (command === "check_database") {
    return { ...mockSession().info, integrity_checked: true, integrity_issues: [] };
  }
  if (command === "export_games") return { path: "/tmp/games-export.pgn", games: 3, bytes: 2048 };
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
    info: {
      games: 1729,
      positions: 110859,
      pgn_bytes: 1311263,
      earliest_date: 20250626,
      latest_date: 20260904,
      results: { white_wins: 813, black_wins: 829, draws: 87, unfinished: 0 },
      integrity_checked: false,
      integrity_issues: [],
    },
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

function mockExplore(player) {
  const detail = mockDetail();
  const board = detail.moves.at(-1).board;
  return {
    player: player ?? null,
    games: 1729,
    players: [
      { name: "QuietKnight", games: 34, wins: 18, draws: 3, losses: 13, unfinished: 0 },
      { name: "CastleCoffee", games: 27, wins: 12, draws: 2, losses: 13, unfinished: 0 },
      { name: "EndgameEnjoyer", games: 21, wins: 9, draws: 4, losses: 8, unfinished: 0 },
    ],
    timeline: [
      { month: 202604, games: 98, wins: 49, draws: 5, losses: 44, unfinished: 0 },
      { month: 202605, games: 121, wins: 63, draws: 7, losses: 51, unfinished: 0 },
      { month: 202606, games: 108, wins: 50, draws: 6, losses: 52, unfinished: 0 },
      { month: 202607, games: 134, wins: 70, draws: 5, losses: 59, unfinished: 0 },
      { month: 202608, games: 146, wins: 71, draws: 9, losses: 66, unfinished: 0 },
      { month: 202609, games: 42, wins: 19, draws: 3, losses: 20, unfinished: 0 },
    ],
    openings: [
      { line: "1. e4 e5 2. Nf3 Nc6", board, games: 286, game_id: 1, ply: 3 },
      { line: "1. d4 Nf6 2. c4 e6", board, games: 201, game_id: 2, ply: 3 },
      { line: "1. e4 c5 2. Nf3 d6", board, games: 184, game_id: 3, ply: 3 },
    ],
    positions: [
      { line: "1. e4 e5 2. Nf3", board, games: 83, game_id: 1, ply: 3 },
      { line: "1. d4 Nf6 2. c4", board, games: 64, game_id: 2, ply: 3 },
      { line: "1. e4 c5 2. Nf3", board, games: 51, game_id: 3, ply: 3 },
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
if (nativeInvoke) initializeNativeApp();
