import test from "node:test";
import assert from "node:assert/strict";
import { acceptSnapshot, sameQueue, scoreText, lossText, feedbackText, fenSquares, summaryText, recommendationLabel, reconcileCoachingProgress } from "./coaching-model.mjs";

test("recommendations report diagnosis and practice rather than legacy opened counts", () => {
  assert.equal(recommendationLabel(null, 6), "Diagnose 6 losses →");
  assert.equal(recommendationLabel({ cancelled: true, games: [{ status: "unseen" }] }, 1), "Continue diagnosis →");
  assert.equal(recommendationLabel({ games: [{ status: "ready", record: { diagnosis: { outcome: { kind: "turning_point" } } } }] }, 1), "Practice 1 position →");
  assert.equal(recommendationLabel({ games: [{ status: "ready" }] }, 1), "View diagnosis summary →");
});

test("cached outcomes repair legacy completion and replay without losing unanalyzed deferrals", () => {
  const progress = { game_ids: [1, 2, 3, 4], reviewed_game_ids: [1, 3], deferred_game_ids: [2, 4] };
  const games = [
    { id: 1, record: { practice: { disposition: "active" } } },
    { id: 2, record: { practice: { disposition: "completed" } } },
    { id: 3, status: "unseen" }, { id: 4, status: "unsupported" },
  ];
  const pending = reconcileCoachingProgress(progress, { running: true, games });
  assert.deepEqual(pending.reviewed_game_ids, [2, 3]);
  const loaded = reconcileCoachingProgress(progress, { running: false, games });
  assert.deepEqual(loaded.reviewed_game_ids, [2]);
  assert.deepEqual(loaded.deferred_game_ids, [4]);
  games[0].record.practice.disposition = "again_later";
  assert.deepEqual(reconcileCoachingProgress(progress, { running: false, games }).deferred_game_ids, [1, 4]);
  assert.deepEqual(progress.reviewed_game_ids, [1, 3]);
});

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
