# Player experience direction

## Product promise

Gambit is the private chess memory that connects a player's games and turns
them into a short, useful review habit.

> Lichess helps you analyze a game. Gambit helps you understand your chess.

The desktop app should not try to replace the place where somebody plays or
the analysis page they open immediately after a game. Its distinctive job is
to answer three questions across a player's history:

1. What changed since I last looked?
2. What pattern is most worth reviewing?
3. Am I improving at it?

## Target player and usage loop

The initial target is a regular online player who has accumulated enough games
that reviewing them one at a time no longer reveals the larger pattern. They
may eventually have games from several online accounts, downloaded PGNs, and
over-the-board events.

The intended loop is:

1. Play wherever the player already plays.
2. Let Gambit bring new games into one local library.
3. Open Gambit once or twice a week and immediately see what changed.
4. Review a small, prioritized set of games or positions.
5. Return after playing more games to see whether the pattern improved.

Success means that opening Gambit leads to a review decision, not merely to
browsing a list of games.

## Experience principles

- **Lead with the next useful action.** Prefer “Review four losses from this
  line” over “This line occurred 37 times.”
- **Show the evidence.** Every recommendation includes its sample size, result
  breakdown, scope, and links to the games behind it.
- **Do not overclaim.** A low-scoring opening or recurring position is a review
  candidate, not necessarily a chess mistake. Only engine-backed analysis may
  label inaccuracies, mistakes, or blunders.
- **Deliver value quickly.** A new Lichess user should be able to inspect recent
  games before a complete historical sync finishes.
- **Keep ownership visible but quiet.** Local storage and privacy should build
  trust without becoming the main task on every screen.
- **Unify the player, not just the files.** The app should eventually understand
  that several usernames and imported PGNs can describe the same person.

## Intended information architecture

### Today

The default destination after sync. It should contain:

- games and results since the previous visit;
- one evidence-backed “work on this” recommendation;
- one positive trend worth reinforcing;
- the next review set, sized in games and estimated minutes;
- recent games as supporting detail rather than the main event.

### Review

A finite queue of games and positions selected from a recommendation. A player
can step through the evidence, open the original game, mark an item reviewed,
and defer it. Local engine analysis can later turn suitable positions into
“find a better move” exercises.

### Library

The current searchable, sortable game browser remains the dependable archive.
It supports detailed inspection and export, but is not the primary return
screen.

### Explore

Longer-term patterns and player-controlled investigation: opening performance,
form over time, opponents, colors, rating bands, time controls, and recurring
positions. Explore should progressively turn frequency tables into comparisons
and review entry points.

## Delivery plan

### Slice 1: one pattern worth reviewing

Use the existing local position index to add a player-scoped focus card to
Explore:

- calculate wins, draws, losses, and unfinished games for common positions
  after move two;
- consider only lines with at least four completed games and at least one loss;
- select the lowest-scoring common line, with deterministic tie-breaking;
- show score, sample size, and loss count;
- open a representative loss at the matching position;
- hide the recommendation when no player identity is in scope or evidence is
  insufficient.

This is deliberately described as a review candidate. It requires no engine
and makes the current Explore data actionable.

### Slice 2: the return moment

Status: in progress. Managed libraries now open on Today, synchronize without
blocking the existing library, and persist the latest successful check with the
new games' player-relative record. Automatic checks are limited to one per 15
minutes and an offline or failed check leaves that local summary intact. A
bounded recent-first initial sync remains before this slice is complete.

- synchronize a managed library automatically on launch, without blocking use
  of the existing database;
- persist the latest successful sync boundary and summary per library;
- add a Today summary for new games and player-relative results;
- prioritize a bounded recent sync so first value does not wait for full
  history, then backfill safely.

### Slice 3: a real review queue

Status: in progress. Opening recommendations now create a focused, bounded
queue of matching losses. The game list shows exactly that set, explains its
scope, stays aligned to the relevant position, and restores the previous
Library state on exit. Players can mark games reviewed, defer them, or open
them on Lichess; progress is stored per library, resumes across launches, and
is summarized on Today. The period comparison remains.

- generate a short queue from the selected pattern;
- preserve progress locally across launches;
- add mark-reviewed, defer, and open-on-Lichess actions;
- compare the recent result for that pattern with the preceding period.

### Slice 4: one player across sources

- store a player profile with multiple aliases;
- associate imported sources with that profile;
- make additional online sources additive rather than separate libraries;
- provide source status, last sync time, and repair/reconnect actions.

### Slice 5: local chess analysis

- run optional local Stockfish analysis outside the interaction thread;
- detect critical evaluation swings across the review set;
- cluster recurring tactical or positional failures only where the evidence
  supports the label;
- create private, repeatable training positions and track recurrence.

## Measures of success

Early product decisions should be evaluated with local, privacy-preserving
signals where possible:

- time from first launch to the first useful recommendation;
- percentage of sessions that start a review;
- number of recommended games reviewed per active week;
- return rate after a completed review;
- percentage of recommendations with enough evidence to be trusted;
- improvement in the selected pattern over a later comparison window.

Raw library size and number of filters used are supporting metrics, not the
primary definition of product value.
