import assert from "node:assert/strict";
import test from "node:test";

import {
  boardCoordinates,
  containedScrollDelta,
  parseSyncDate,
  perspectivePlayerIsBlack,
  timelineProgress,
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
