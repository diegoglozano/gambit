import assert from "node:assert/strict";
import test from "node:test";

import {
  boardCoordinates,
  containedScrollDelta,
  createRequestGate,
  formatExploreMonth,
  formatPlayerRecord,
  parseSyncDate,
  perspectivePlayerIsBlack,
  nextReviewGameId,
  prepareReviewProgress,
  reconcileReviewProgress,
  reviewPatternKey,
  reviewProgressMatches,
  reviewSummaries,
  selectFocusOpening,
  timelineProgress,
  validateLiveFilters,
} from "./view-model.mjs";

test("board coordinates reverse ranks and files for Black", () => {
  const white = boardCoordinates(false);
  const black = boardCoordinates(true);
  assert.deepEqual(white[0], { rank: 7, file: 0, row: 0, column: 0 });
  assert.deepEqual(white[63], { rank: 0, file: 7, row: 7, column: 7 });
  assert.deepEqual(black[0], { rank: 0, file: 7, row: 0, column: 0 });
  assert.deepEqual(black[63], { rank: 7, file: 0, row: 7, column: 7 });
});

test("perspective follows the filtered or managed player case-insensitively", () => {
  assert.equal(perspectivePlayerIsBlack("diegoglozano", null, "DiegoGlozano"), true);
  assert.equal(perspectivePlayerIsBlack(null, "diegoglozano", "DiegoGlozano"), true);
  assert.equal(perspectivePlayerIsBlack("Opponent", "diegoglozano", "DiegoGlozano"), false);
});

test("move navigation only requests movement inside its scroll container", () => {
  const container = { top: 100, bottom: 300 };
  assert.equal(containedScrollDelta(container, { top: 130, bottom: 160 }), 0);
  assert.equal(containedScrollDelta(container, { top: 80, bottom: 110 }), -28);
  assert.equal(containedScrollDelta(container, { top: 290, bottom: 330 }), 38);
});

test("history progress follows dates but remains below completion while downloading", () => {
  const start = parseSyncDate("2025-01-01");
  const middle = parseSyncDate("2025.07.02");
  const end = parseSyncDate("2026-01-01");
  assert.ok(timelineProgress(start, end, middle) > 49);
  assert.ok(timelineProgress(start, end, middle) < 51);
  assert.equal(timelineProgress(start, end, end), 98);
  assert.equal(timelineProgress(null, end, middle), null);
});

test("only the latest asynchronous request remains current", () => {
  const gate = createRequestGate();
  const first = gate.next();
  const second = gate.next();
  assert.equal(gate.isCurrent(first), false);
  assert.equal(gate.isCurrent(second), true);
  gate.invalidate();
  assert.equal(gate.isCurrent(second), false);
});

test("live filters wait for complete dependent and range values", () => {
  assert.match(validateLiveFilters({ opponent: "Other" }), /Choose a player/);
  assert.match(validateLiveFilters({ position: "8/8/8/8/8/8/8/8 w" }), /six-field FEN/);
  assert.match(validateLiveFilters({ since: "2026-09-02", until: "2026-09-01" }), /start date/);
  assert.match(validateLiveFilters({ player: "Alice", minimum_rating: "1500", maximum_rating: "1400" }), /minimum rating/);
  assert.equal(validateLiveFilters({ player: "Alice", opponent: "Bob" }), null);
  assert.equal(validateLiveFilters({ position: "8/8/8/8/8/8/8/8 w - - 0 1" }), null);
});

test("explore months use compact labels", () => {
  assert.equal(formatExploreMonth(202609), "Sep ’26");
  assert.equal(formatExploreMonth(0), "0");
});

test("focus opening selects the lowest supported player score", () => {
  const focus = selectFocusOpening([
    { line: "A", wins: 3, draws: 0, losses: 1 },
    { line: "B", wins: 1, draws: 1, losses: 4 },
    { line: "C", wins: 0, draws: 0, losses: 3 },
  ]);
  assert.equal(focus.line, "B");
  assert.equal(focus.completed, 6);
  assert.equal(focus.score, 25);
});

test("focus opening requires enough completed evidence and a loss", () => {
  assert.equal(selectFocusOpening([{ line: "Too small", wins: 0, draws: 0, losses: 3 }]), null);
  assert.equal(selectFocusOpening([{ line: "Unbeaten", wins: 4, draws: 1, losses: 0 }]), null);
});

test("player record stays compact and hides an empty result set", () => {
  assert.equal(formatPlayerRecord({ wins: 12, draws: 3, losses: 9 }), "12W · 3D · 9L");
  assert.equal(formatPlayerRecord({ unfinished: 2 }), null);
});

test("review summaries contain only queued games in queue order", () => {
  const details = [
    { summary: { id: 3, date: "2026.09.03" } },
    { summary: { id: 9, date: "2026.09.09" } },
    { summary: { id: 5, date: "2026.09.05" } },
  ];

  assert.deepEqual(reviewSummaries([9, 3], details).map((game) => game.id), [9, 3]);
});

test("review progress is reconciled with the current recommendation", () => {
  const opening = { line: "1. d4 d5 2. Bf4 Nf6", ply: 4, losses: 58, review_game_ids: [9, 7, 5] };
  const saved = {
    pattern: reviewPatternKey(opening, "Diego"),
    reviewed_game_ids: [9, 100, 9],
    deferred_game_ids: [7, 9],
    current_game_id: 7,
  };

  const progress = reconcileReviewProgress(opening, saved, "diego");
  assert.equal(reviewProgressMatches(opening, saved, "DIEGO"), true);
  assert.equal(reviewProgressMatches(opening, saved, "another-player"), false);
  assert.deepEqual(progress.game_ids, [9, 7, 5]);
  assert.deepEqual(progress.reviewed_game_ids, [9]);
  assert.deepEqual(progress.deferred_game_ids, [7]);
  assert.equal(progress.current_game_id, 7);
  assert.equal(progress.matching_losses, 58);
});

test("starting again resets completed work and reopens a fully deferred queue", () => {
  const opening = { line: "Line", ply: 4, losses: 2, review_game_ids: [1, 2] };
  const completed = reconcileReviewProgress(opening, null);
  completed.reviewed_game_ids = [1, 2];
  assert.deepEqual(prepareReviewProgress(opening, completed).reviewed_game_ids, []);

  const deferred = reconcileReviewProgress(opening, null);
  deferred.deferred_game_ids = [1, 2];
  assert.deepEqual(prepareReviewProgress(opening, deferred).deferred_game_ids, []);

  const partiallyDeferred = reconcileReviewProgress(opening, null);
  partiallyDeferred.deferred_game_ids = [1];
  partiallyDeferred.current_game_id = 1;
  assert.equal(prepareReviewProgress(opening, partiallyDeferred).current_game_id, 2);
});

test("review advancement skips reviewed and deferred games", () => {
  const progress = {
    game_ids: [1, 2, 3, 4],
    reviewed_game_ids: [2],
    deferred_game_ids: [3],
  };
  assert.equal(nextReviewGameId(progress, 1), 4);
  progress.reviewed_game_ids.push(4);
  assert.equal(nextReviewGameId(progress, 1), 1);
  progress.reviewed_game_ids.push(1);
  assert.equal(nextReviewGameId(progress, 1), null);
});
