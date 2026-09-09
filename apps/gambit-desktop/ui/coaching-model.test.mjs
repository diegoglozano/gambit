import test from "node:test";
import assert from "node:assert/strict";
import { acceptSnapshot, sameQueue, scoreText, lossText, feedbackText, fenSquares, summaryText } from "./coaching-model.mjs";

test("stale library, generation and revision cannot replace current progress", () => {
  const current = { path: "a", generation: 2, revision: 4 };
  for (const incoming of [{ path: "b", generation: 3 }, { path: "a", generation: 1 }, { ...current, revision: 3 }]) {
    assert.equal(acceptSnapshot(current, incoming, "a"), current);
  }
  assert.equal(acceptSnapshot(current, { ...current, revision: 5 }, "a").revision, 5);
});
test("queue identity includes explicit player, shared ply and ordered game IDs", () => {
  const context = { path: "a", player: "Player", ply: 4, gameIds: [1, 2] };
  const snapshot = { path: "a", player: "player", shared_ply: 4, games: [{ id: 1 }, { id: 2 }] };
  assert.equal(sameQueue(snapshot, context), true);
  for (const changed of [{ player: "other" }, { shared_ply: 5 }, { games: [{ id: 2 }, { id: 1 }] }]) {
    assert.equal(sameQueue({ ...snapshot, ...changed }, context), false);
  }
});
test("factual scores retain inequalities and typed mates", () => {
  assert.equal(scoreText({ bound: "lower", score: { kind: "centipawns", value: -34 } }), "at least -0.34 pawns");
  assert.equal(scoreText({ bound: "exact", score: { kind: "mate_against", value: 0 } }), "mate against you");
  assert.equal(lossText({ kind: "centipawns", minimum: 100, exact: false }), "at least 1.00 pawns of evaluation lost");
  assert.match(feedbackText("inconclusive"), /inconclusive/);
  assert.doesNotMatch(feedbackText("inconclusive"), /Try again/);
});
test("practice board uses exact FEN squares, not the game viewer position", () => {
  const squares = fenSquares("7k/5K2/6Q1/8/8/8/8/8 w - - 0 1");
  assert.equal(squares.length, 64);
  assert.equal(squares.find((s) => s.name === "g6").piece, "Q");
  assert.equal(squares.find((s) => s.name === "h8").piece, "k");
});
test("empty summary does not invent evidence", () => {
  assert.equal(summaryText(null), "Completed results are saved privately on this Mac.");
});
