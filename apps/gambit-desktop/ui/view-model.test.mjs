import assert from "node:assert/strict";
import test from "node:test";

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
