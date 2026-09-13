import test from "node:test";
import assert from "node:assert/strict";
import { learningFlow, lessonFinding, lessonBoard } from "./learning-model.mjs";

const ctx = { path: "library", player: "A" };
const games = [{ id: 1 }, { id: 2 }];
const snapshot = (overrides = {}) => ({ path: "library", player: "A", shared_ply: 0, generation: 1, revision: 1,
  running: false, games: games.map(game => ({ ...game, status: "unseen" })), ...overrides });

test("preparation selects games and starts evidence without a review queue or manual diagnosis", async () => {
  const calls = [];
  let current = null;
  const flow = learningFlow({ setTimer: () => 1, clearTimer() {}, onUpdate() {}, async invoke(command, args) {
    calls.push({ command, args });
    if (command === "lesson_games") return games;
    if (command === "coaching_status") return current;
    if (command === "coaching_overview") return snapshot();
    if (command === "start_coaching") { current = snapshot({ running: args.request.analyze, generation: 4 }); return current; }
  } });
  await flow.prepare(ctx);
  assert.equal(calls.find(call => call.command === "start_coaching").args.request.analyze, true);
  assert.deepEqual(calls.find(call => call.command === "start_coaching").args.request.game_ids, [1, 2]);
  const count = calls.length;
  await flow.prepare(ctx);
  assert.equal(calls.length, count); // Repeated Today renders never duplicate a job.
  flow.receive(snapshot({ generation: 3, running: false }));
  assert.equal(flow.current().snapshot.generation, 4);
});

test("interrupted preparation loads cached lessons, then waits for explicit resume", async () => {
  const searches = [];
  let current = null;
  const flow = learningFlow({ setTimer: () => 1, onUpdate() {}, async invoke(command, args) {
    if (command === "lesson_games") return games;
    if (command === "coaching_status") return current;
    if (command === "coaching_overview") return snapshot({ cancelled: true });
    if (command === "start_coaching") { searches.push(args.request.analyze); return current = snapshot({ generation: searches.length + 1 }); }
  } });
  await flow.prepare(ctx);
  assert.deepEqual(searches, [false]);
  assert.equal(flow.current().paused, true);
  await flow.prepare(ctx, { resume: true });
  assert.deepEqual(searches, [false, true]);
  assert.equal(flow.current().paused, false);
});

test("entering a lesson while candidate loading cannot replace its current service", async () => {
  let release;
  let blocked = false;
  const calls = [];
  const flow = learningFlow({ onUpdate() {}, blocked: () => blocked, invoke(command) {
    calls.push(command);
    if (command === "lesson_games") return new Promise(resolve => { release = resolve; });
    assert.fail("late preparation must not mutate the lesson service");
  } });
  const pending = flow.prepare(ctx);
  blocked = true;
  release(games);
  await pending;
  assert.deepEqual(calls, ["lesson_games"]);
});

test("empty games and engine failures offer state without inventing a lesson", async () => {
  const flow = learningFlow({ onUpdate() {}, async invoke() { return []; } });
  await flow.prepare(ctx);
  assert.equal(lessonFinding(flow.current().snapshot), null);
  flow.reset();
  const failed = learningFlow({ onUpdate() {}, async invoke() { throw new Error("unavailable"); } });
  await failed.prepare(ctx);
  assert.match(failed.current().error, /Resume.*browse/);
  assert.equal(failed.current().loading, false);
});

test("findings prioritize available evidence and retain concrete recurrence scope", () => {
  const game = (id, disposition = "active", minimum = 200, fen = "7k/5K2/6Q1/8/8/8/8/8 w - - 0 1") => ({ id, record: {
    practice: { disposition, solution: "unsolved", revealed: false }, diagnosis: { outcome: { kind: "turning_point", evidence: {
      position_fen: fen, played_uci: "g6h7", loss: { kind: "centipawns", minimum, exact: true },
    } } },
  } });
  const finding = lessonFinding(snapshot({ games: [game(1), game(2, "completed", 900), game(3)] }), [{ id: 1 }, { id: 2 }, { id: 3 }]);
  assert.equal(finding.game.id, 1);
  assert.deepEqual(finding.supportingIds, [1, 2, 3]);
  assert.equal(finding.revisit, false);
  const board = lessonBoard(finding.point.position_fen);
  assert.equal(board.length, 64);
  assert.equal(board[6 * 8 + 5], "K");
  assert.equal(board[5 * 8 + 6], "Q");
  assert.equal(lessonFinding(snapshot()), null);
});
