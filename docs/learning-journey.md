# Games to a lesson: delivery and local QA

## Baseline (v0.17.0, September 13)

Native inspection of the returning-player app found a six-game opening review
set, previous/next/defer/exit actions, a move preview waiting for Check my move,
and a separate Done action. Today leads through a review set and explicit
Analyze review set before a first uncached exercise. The fresh-user code path
blocks on the initial export and index, then uses that same recommendation.
This is an inspection baseline, not a timed participant study.

## Intended journey

Connect a username with a short explanation of bounded, private local analysis.
Show usable games while preparing a small sample of recent player decisions.
Today prioritizes one supported mistake with a position preview and source game;
opening performance remains an investigation, not evidence of a mistake.
Enter the available lesson with one action. Select a piece, then complete a
legal destination by click, drop, or keyboard activation to grade it. Promotion
waits only for a piece choice. Keep the original move and better legal
continuation visually accessible; supporting engine details are optional.
Save solved/revealed outcomes automatically. Keep feedback and playback in place
until Continue or exit. Save for later and skipped work remain distinct from
independent solves. Return to the originating view or inspect the source game.

Preparation has fixed internal limits, loads cache before searches, and offers
pause/resume and useful browsing while no lesson is ready. Failure and bounded
no-result states do not claim that the player's games were mistake-free.
Background arrivals must preserve board selection, drag, feedback, and playback.

## Verification record

Each feature PR records its relevant controller, browser, backend and native
checks. A release needs native real-engine validation of the assembled journey.
No private game data, positions, or analysis reports belong in PR attachments.
Technical checks cannot establish learning comprehension; participant research
must be explicitly identified when it has actually happened.

Direct submission native QA confirmed selection without attempts, automatic
grading on destination, rejected-move restoration, and feedback preservation.
An isolated native returning-player launch showed automatic preparation on
Today without any game-selection or diagnosis action. Controller tests cover
interruption, cache reuse, late preparation while a lesson opens, and honest
empty/failure states; browser checks enter from Today with one action.
