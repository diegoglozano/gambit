# Player experience and product direction

Updated September 13, 2026, after v0.17.0. This is the product brief and handoff
for the next development loop. It supersedes the previous delivery plan on this
page. It describes intended behavior; the [desktop guide](desktop.md) describes
what is available today.

## Product promise

Gambit should become a **free, smart chess academy built around the player's
own games**: understand a problem, practice a correction, and see whether it
improves in later games.

The core question is: **What should I learn from my games, and how do I stop
making the same mistake?**

The database, search, and analysis board are foundations for that promise.
The user cites alternatives such as En Croissant as a reason to focus on
differentiation through learning; this brief makes no comparative feature
claims. More database controls and more engine output are not sufficient
product outcomes.

The initial player is a regular Lichess user who wants to improve but does not
know opening positions, notation, or engine variations by heart. They should
learn by looking at and moving pieces on a board.

## The learning loop

1. **Ingest games.** Connect a Lichess username once. Fetch new games and
   maintain the local library automatically. Chess.com is a future source that
   should feed the same player history and learning loop.
2. **Navigate and search.** Preserve the existing game browser, filters, search,
   and replay. The user considers this broadly satisfactory; it is not the
   next redesign priority.
3. **Explore and understand.** Investigate openings and recurring positions,
   and, most importantly, supported errors and repeated incorrect decisions.
   Prepare useful findings without making the player assemble a review queue
   and start several separate jobs.
4. **Learn a correction.** Show a concrete position from the player's games,
   what they played, what went wrong, and a better continuation. Let them try
   directly on the board, with short visual feedback.
5. **Revisit and improve.** Bring useful corrections back for practice and
   look for recurrence in later games. Solving an exercise is practice
   progress; fewer supported errors in later games is evidence of improvement.

The intended first journey is: enter a Lichess username, see recent games while
analysis continues, open one useful finding, and learn from the board. The
player should not need to understand internal analysis states to get value.

## Analysis priorities

### Openings and patterns

Show the lines the player actually reaches, their results, recurring positions,
and where games diverge. Use board previews and readable opening names when
available, with notation and tables on demand.

A low score in an opening is a reason to investigate, not proof that the opening
or a move is wrong. Keep performance observations separate from supported
errors.

### Errors: highest priority

Help the player answer:

- Where do I usually make consequential mistakes?
- Do I repeatedly choose the same bad move in the same position?
- Does a similar, supported problem occur in different games?
- What should I notice next time, and what should I do differently?

Use local Stockfish analysis as evidence behind the experience. Lead with the
board and plain language. Evaluation numbers, depth, budgets, and principal
variations are supporting details, not required knowledge or routine decisions.

Start with defensible recurrence: the same playable position and played choice
across games. Broader tactical or positional themes need a validated way to
identify them. Do not infer labels such as missed forks or weak king safety
from a centipawn swing alone. When a theme is not established, show the concrete
mistake and legal continuation honestly.

Prioritize findings using consequence, recurrence, recency, and confidence.
Show the supporting sample and source games. A single mistake can be worth
teaching, but must not be presented as a habit. No supported result at a bounded
analysis budget does not mean the player made no mistakes.

## Simplify the workflow

The current experience asks the player to make selections and then push
buttons through too many stages. v0.17.0 improved visual practice; the next
work must simplify the journey itself.

| Current friction | Intended experience |
| --- | --- |
| Choose a pattern, open a review set, start diagnosis, then enter practice | One entry into a useful lesson; the app prepares evidence |
| Decide whether to diagnose, continue diagnosis, practice, or view a summary | One contextual action based on readiness |
| Select a move, then press Check my move | Completing a legal board move submits the attempt |
| Repeatedly mark reviewed, done, or next to maintain progress | Save progress automatically, with a clear continuation or exit |
| Read coordinates and variations to understand a position | Board previews, arrows, short playback, and named-piece guidance |
| Wait for a whole import or analysis set | Existing games stay usable; findings appear progressively |

Automatic submission needs an intentional gesture: piece selection alone is
not an attempt; completing a destination by click or drop is. Promotion asks
only for the piece choice. Prevent duplicate submissions, show evaluation
progress, and offer clear retry/recovery. Explanation playback and exploratory
moves must not accidentally become graded attempts. Preserve an accessible
keyboard path.

Automatic progress must distinguish solved without help, answer revealed,
skipped, and saved for later. Opening a lesson is not solving it. After feedback,
let the player inspect the board at their own pace rather than immediately
replacing it with the next exercise.

## UX principles

- **One useful next step.** One clear primary action per learning screen;
  secondary controls appear when relevant.
- **Automate preparation.** Sync, bounded analysis, caching, and lesson
  selection happen behind the workflow. Explain local analysis at setup;
  resource controls and pause/cancel remain accessible.
- **Teach visually.** Show the position, consequence, and correction without
  requiring FEN, SAN, UCI, or memorized positions.
- **Explain honestly.** Show evidence and uncertainty. A legal visual
  continuation is preferable to an unsupported explanation of a chess concept.
- **Preserve attention.** Background work must not switch the current game,
  reset a move, clear feedback, or interrupt an explanation.
- **Keep lessons small.** A short, useful lesson is better than an exhaustive
  dashboard or a large unfinished queue.
- **Stay free and local-first.** The learning loop should not depend on a paid
  coaching service or uploading games to Gambit. Sync communicates directly
  with the chosen source.

## Roles of the existing destinations

**Today** is the learning home: one prioritized finding, a short practice
continuation, and later evidence of progress. Sync/analysis status is useful,
quiet context rather than a dashboard of competing actions.

**Library** remains the searchable archive and access to source games.
Preserve its working navigation and filters.

**Explore** supports player-led investigation of openings, patterns, and
errors. Findings lead into the same learning flow as Today.

**Practice/review** is the lesson experience, not an administrative queue to
manage. Returning to the source game and leaving a lesson remain easy.

## Starting point: v0.17.0

Available foundations include Lichess ingestion and background checks, indexed
search, game replay, opening/recurring-position summaries, bounded local
turning-point diagnosis, durable practice outcomes, visual move previews, and
answer playback. See [local turning-point evidence](review-diagnosis.md) for
the engine and cache contracts.

Important gaps:

- Diagnosis is reached through selected review sets and explicit controls,
  rather than an automatic learning pipeline over newly imported games.
- Recommendations start from opening results; error-first lesson selection
  is the intended priority.
- Legal answer lines and move descriptions do not yet establish reliable
  explanations of the underlying chess concept.
- Current summaries group identical positions/choices within bounded sets.
  They do not establish library-wide themes, spaced repetition, or measured
  long-term improvement.
- Practice still requires explicit checking and several progress actions.

Keep legal replay, authoritative backend grading, cancellation, cache validity,
evidence bounds, and privacy protections. Simplifying the interface should reuse
these services rather than bypass them.

## Next development loop

Deliver **one simple path from the player's games to a useful lesson**.

1. Walk through a fresh Lichess user and a returning user in the native app.
   Record necessary decisions, clicks, and waiting from import to first lesson
   as the baseline.
2. Design the complete journey before changing individual controls: one lesson
   entry, progressive preparation, direct board attempt, visual feedback,
   saved progress, and a clear return path.
3. Implement bounded automatic preparation using existing diagnosis services.
   Define scheduling/resource budgets internally, reuse valid cache, and make
   interruption/resumption understandable.
4. Prefer a supported error when selecting the first lesson. Show its source
   game and concrete correction. If none is ready, offer useful progress or
   browsing rather than an empty training funnel.
5. Remove redundant confirmation and queue-management steps. Verify direct
   move submission, reveal, retry, continuation, and leaving mid-lesson.
6. Validate the journey natively with real engine evidence and a player who
   does not rely on notation. Technical test passes alone do not establish
   that the learning experience works.

### Acceptance criteria

- After entering a Lichess username, the player need not select games, build a
  review set, or start diagnosis manually to receive a first lesson.
- A returning player enters an available lesson from Today with one action.
- Games and cached lessons remain usable while background work runs.
- Completing a legal move grades it without a separate Check action; selecting
  a piece or playing an answer line never records an unintended attempt.
- The original decision, supported consequence, and correction are
  understandable on the board without notation or engine numbers.
- Progress survives exit/relaunch; revealed or skipped work is not counted as
  an independent solve.
- Failed, cancelled, unsupported, insufficient-evidence, and no-result states
  have clear recovery or alternatives without engine expertise.
- Background results preserve the current interaction; the flow works at the
  minimum supported window size and through keyboard input.

## Later work and boundaries

Once the simple lesson flow works, expand recurrence across the library,
validate broader problem themes, add appropriate repeat practice, and compare
later opportunities with earlier ones. Add Chess.com to the same player model
when that source is implemented.

Chess.com, a full opening repertoire, generic course content, full-game
annotation, a theme taxonomy, and a conversational coach are not prerequisites
for the next slice. Do not redesign satisfactory Library/search functionality
without evidence of a learning-flow problem. This brief documents direction;
it does not implement the new workflow or require another binary release.

## Measures of success

Use representative local QA or explicitly opt-in research. These measures
do not require uploading private games or silent telemetry.

- Time and user decisions from first import to the first useful lesson.
- Time and actions from returning to the app to beginning practice.
- Whether a player can explain the problem and what to notice next time.
- Completion and return to short lessons, distinguishing help from independent
  solves.
- Recurrence of supported errors in later comparable opportunities, with
  sample size, scope, and uncertainty visible.

Library size, filters used, and completed engine jobs are operational signals.
Product success is the player's understanding and improvement.
