import test from "node:test";
import assert from "node:assert/strict";
import { coachingUI } from "./coaching-ui.mjs";

// A deliberately small DOM test double: this tests controller state and event
// contracts, not browser layout, native accessibility, or rendering quality.
class Element {
  children = []; listeners = {}; dataset = {}; attributes = {}; hidden = false; textContent = ""; value = "";
  style = {}; classes = new Set();
  classList = { toggle: (name, active) => active ? this.classes.add(name) : this.classes.delete(name) };
  setAttribute(name, value) { this.attributes[name] = value; }
  addEventListener(name, listener) { this.listeners[name] = listener; }
  append(child) { this.children.push(child); }
  replaceChildren() { this.children = []; }
  contains(child) { return this.children.includes(child); }
  querySelectorAll() { return this.children; }
  querySelector(selector) { return this.children.find((child) => selector.includes(child.dataset.square)); }
  focus() { document.activeElement = this; }
  closest() { return this; }
}

test("practice controller hides the answer, supports square entry and reveals only on acceptance", async () => {
  const previousDocument = globalThis.document;
  const previousWindow = globalThis.window;
  const elements = new Map();
  const element = (id) => { if (!elements.has(id)) elements.set(id, new Element()); return elements.get(id); };
  globalThis.document = { activeElement: null, getElementById: element, createElement: () => new Element() };
  globalThis.window = { setTimeout: () => 1 };
  try {
    let ctx = { path: "library", player: "A", ply: 4, gameIds: [1], gameId: 1 };
    const record = { diagnosis: { outcome: { kind: "turning_point", evidence: {
      position_fen: "7k/5K2/6Q1/8/8/8/8/8 w - - 0 1", played_san: "Qh7+", best_san: "Qg8#", pv_san: ["Qg8#"],
      before: { bound: "exact", score: { kind: "mate_for", value: 1 } },
      after: { bound: "exact", score: { kind: "centipawns", value: 0 } }, loss: { kind: "lost_forced_mate" },
    } } }, practice: { solution: "unsolved", revealed: false, attempts: [] } };
    const snapshot = { path: "library", player: "A", shared_ply: 4, generation: 1, revision: 1,
      running: false, games: [{ id: 1, status: "ready", record, move_options: [
        { uci: "g6h6", fen: "7k/5K2/7Q/8/8/8/8/8 b - - 1 1" },
        { uci: "g6g8", fen: "6Qk/5K2/8/8/8/8/8/8 b - - 1 1" },
      ], line_positions: [
        "7k/5K2/6Q1/8/8/8/8/8 w - - 0 1", "6Qk/5K2/8/8/8/8/8/8 b - - 1 1",
      ] }] };
    const calls = [];
    let statusReads = 0;
    const ui = coachingUI({ context: () => ctx, onDone() {}, onLater() {}, invoke: async (command, args) => {
      calls.push({ command, args });
      if (command === "coaching_status" && statusReads++ === 0) return null;
      if (command === "coaching_practice") {
        if (args.action.kind === "reveal") record.practice.revealed = true;
        else {
          assert.deepEqual(args.action, { kind: "attempt", uci: "g6h6" });
          record.practice.solution = "without_reveal";
          record.practice.attempts.push({ verdict: "strong" });
        }
        snapshot.revision++;
      }
      return structuredClone(snapshot);
    } });
    await ui.load();
    assert.equal(element("coaching-feedback").textContent, "");
    assert.equal(element("coaching-answer").hidden, true);
    assert.equal(element("coaching-answer").textContent, "");
    assert.equal(element("coaching-pv").textContent, "");
    assert.equal(element("coaching-playback").hidden, true);
    assert.equal(element("coaching-done").disabled, true);
    const board = element("coaching-board");
    assert.equal(board.children.length, 64);
    assert.equal(board.children.filter((b) => b.tabIndex === 0).length, 1);
    const square = name => board.children.find(b => b.dataset.square === name);
    document.elementFromPoint = () => square("h6");
    square("g6").listeners.pointerdown({ pointerId: 1, button: 0, clientX: 10, clientY: 10, preventDefault() {} });
    board.listeners.pointermove({ pointerId: 1, clientX: 40, clientY: 10 });
    assert.equal(element("coaching-drag-piece").hidden, false);
    board.listeners.pointerup({ pointerId: 1, clientX: 40, clientY: 10 });
    assert.equal(element("coaching-drag-piece").hidden, true);
    assert.equal(element("coaching-move").value, "g6h6");
    assert.equal(square("g6").attributes["aria-label"], "g6, empty");
    assert.equal(square("h6").attributes["aria-label"], "h6, White queen");
    assert.equal(square("h6").attributes["aria-disabled"], "true");
    assert.equal(calls.filter(c => c.command === "coaching_practice").length, 0);
    element("coaching-reset").listeners.click();
    assert.equal(square("g6").attributes["aria-label"], "g6, White queen");
    assert.equal(element("coaching-move").value, "");
    square("g6").listeners.pointerdown({ pointerId: 2, button: 0, clientX: 10, clientY: 10, preventDefault() {} });
    board.listeners.pointermove({ pointerId: 2, clientX: 40, clientY: 10 });
    board.listeners.pointercancel();
    assert.equal(element("coaching-drag-piece").hidden, true);
    assert.equal(element("coaching-move").value, "");
    assert.equal(square("g6").classes.has("drag-origin"), false);
    document.elementFromPoint = () => null;
    square("g6").listeners.pointerdown({ pointerId: 3, button: 0, clientX: 10, clientY: 10, preventDefault() {} });
    board.listeners.pointermove({ pointerId: 3, clientX: 500, clientY: 10 });
    board.listeners.pointerup({ pointerId: 3, clientX: 500, clientY: 10 });
    assert.equal(element("coaching-move").value, "");
    square("h8").listeners.click(); // Opponent/empty squares cannot start a move.
    assert.equal(square("h8").attributes["aria-pressed"], "false");
    let stopped = false;
    board.children[0].listeners.keydown({ key: "ArrowRight", preventDefault() {}, stopPropagation() { stopped = true; } });
    assert.equal(stopped, true);
    assert.equal(document.activeElement, board.children[1]);
    assert.equal(board.children.filter((b) => b.tabIndex === 0).length, 1);
    board.children.find((b) => b.dataset.square === "g6").listeners.click();
    assert.equal(square("h6").classes.has("legal-target"), true);
    square("a1").listeners.click();
    assert.equal(element("coaching-move").value, "");
    assert.match(element("coaching-feedback").textContent, /not legal/);
    board.children.find((b) => b.dataset.square === "h6").listeners.click();
    assert.equal(element("coaching-move").value, "g6h6");
    assert.equal(calls.filter((c) => c.command === "coaching_practice").length, 0);
    assert.equal(square("h6").attributes["aria-label"], "h6, White queen");
    element("coaching-form").listeners.submit({ preventDefault() {} });
    await new Promise(setImmediate);
    assert.equal(element("coaching-answer").hidden, false);
    assert.match(element("coaching-answer").textContent, /Qg8#/);
    assert.match(element("coaching-feedback").textContent, /Strong move/);
    assert.equal(element("coaching-done").disabled, false);
    element("coaching-reveal").listeners.click();
    await new Promise(setImmediate);
    assert.equal(element("coaching-feedback").textContent, "Progress saved on this Mac.");
    assert.equal(element("coaching-playback").hidden, false);
    element("coaching-line-next").listeners.click();
    assert.match(element("coaching-line-status").textContent, /Qg8#/);
    assert.equal(element("coaching-line-next").disabled, true);
    assert.equal(element("coaching-check").disabled, true);
    assert.equal(board.children.find(b => b.dataset.square === "g8").attributes["aria-label"], "g8, White queen");
    // Orientation remains White's even though the line's side to move is Black.
    assert.equal(board.children[0].dataset.square, "a8");
    const beforePlaybackSubmit = calls.length;
    element("coaching-form").listeners.submit({ preventDefault() {} });
    assert.equal(calls.length, beforePlaybackSubmit);
    element("coaching-line-start").listeners.click();
    assert.equal(element("coaching-check").disabled, false);
    assert.equal(board.children.find(b => b.dataset.square === "g6").attributes["aria-label"], "g6, White queen");
    ctx = { ...ctx, path: "other-library" };
    ui.render();
    assert.equal(element("coaching-exercise").hidden, true);
    assert.equal(element("coaching-answer").textContent, "");
    ctx = { ...ctx, path: "library" };
    for (const status of ["unseen", "analyzing", "unsupported", "failed"]) {
      ui.receive({ ...snapshot, revision: ++snapshot.revision, running: status === "analyzing",
        games: [{ id: 1, status, message: status === "failed" ? "Retry this game" : null }] });
      assert.equal(element("coaching-exercise").hidden, true);
      assert.equal(element("coaching-answer").textContent, "");
      assert.equal(element("coaching-done").hidden, true);
      if (status === "failed") assert.equal(element("coaching-status").textContent, "Retry this game");
    }
    ui.receive({ ...snapshot, revision: ++snapshot.revision, running: false, cancelled: true,
      games: [{ id: 1, status: "ready", record: { diagnosis: { outcome: { kind: "no_clear_turning_point" }, inconclusive_moves: 1 },
        practice: { solution: "unsolved", revealed: false } } }] });
    assert.match(element("coaching-status").textContent, /No clear turning point/);
    assert.match(element("coaching-status").textContent, /inconclusive/);
    assert.match(element("coaching-progress").textContent, /paused/);
    assert.equal(element("coaching-done").disabled, false);
  } finally { globalThis.document = previousDocument; globalThis.window = previousWindow; }
});

test("Black promotion, text preview and busy guards keep backend-provided boards and orientation", () => {
  const previousDocument = globalThis.document;
  const previousWindow = globalThis.window;
  const elements = new Map();
  const element = id => { if (!elements.has(id)) elements.set(id, new Element()); return elements.get(id); };
  globalThis.document = { activeElement: null, getElementById: element, createElement: () => new Element() };
  globalThis.window = { setTimeout: () => 1 };
  try {
    const root = "4k3/8/8/8/8/8/1p6/4K3 b - - 0 9";
    const knight = "4k3/8/8/8/8/8/8/1n2K3 w - - 0 10";
    const record = { diagnosis: { outcome: { kind: "turning_point", evidence: {
      position_fen: root, played_san: "b1=Q+", pv_san: [],
    } } }, practice: { solution: "unsolved", revealed: false } };
    const snapshot = { path: "library", player: "B", shared_ply: 0, generation: 1, revision: 1, running: false,
      games: [{ id: 2, record, status: "ready", move_options: [
        { uci: "b2b1q", fen: "4k3/8/8/8/8/8/8/1q2K3 w - - 0 10" }, { uci: "b2b1n", fen: knight },
      ] }] };
    const ui = coachingUI({ context: () => ({ path: "library", player: "B", ply: 0, gameIds: [2], gameId: 2 }),
      invoke() { assert.fail("preview must not invoke the engine or save an attempt"); }, onDone() {}, onLater() {} });
    element("coaching-promotion").value = "q";
    ui.receive(snapshot);
    const board = element("coaching-board");
    const square = name => board.children.find(child => child.dataset.square === name);
    assert.equal(board.children[0].dataset.square, "h1");
    square("b2").listeners.click(); square("b1").listeners.click();
    assert.equal(square("b1").attributes["aria-label"], "b1, Black queen");
    element("coaching-promotion").value = "n";
    element("coaching-promotion").listeners.change();
    assert.equal(element("coaching-move").value, "b2b1n");
    assert.equal(square("b1").attributes["aria-label"], "b1, Black knight");
    assert.equal(board.children[0].dataset.square, "h1");
    element("coaching-reset").listeners.click();
    assert.equal(square("b2").attributes["aria-label"], "b2, Black pawn");
    element("coaching-move").value = "b2b1n"; element("coaching-move").listeners.input();
    assert.equal(square("b1").attributes["aria-label"], "b1, Black knight");
    element("coaching-reset").listeners.click();
    ui.receive({ ...snapshot, revision: 2, practice_busy: true });
    square("b2").listeners.click();
    assert.equal(square("b2").attributes["aria-pressed"], "false");
    assert.equal(element("coaching-move").disabled, true);
  } finally { globalThis.document = previousDocument; globalThis.window = previousWindow; }
});
