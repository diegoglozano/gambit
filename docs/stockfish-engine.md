# Local engine spike (Phase 0)

This implements the first deliverable from `review-coaching-requirements.md`:
a backend-only UCI boundary and a packaged fixed-position diagnostic. It does
not implement diagnosis, cache schema, training, or coaching UI. Those remain
gated on the profiling and distribution checks below.

## Engine and provenance

Pinned release: [Stockfish 17.1](https://github.com/official-stockfish/Stockfish/releases/tag/sf_17.1),
released March 30, 2025. This is a deliberate stable build selection, not a claim
that it is the newest engine. Upstream binaries are unmodified except for joining
their Mach-O slices with `lipo` and app signing. Intel uses baseline x86-64,
not AVX2/BMI2, to avoid an unnecessary CPU requirement.

Files are fetched over HTTPS by `scripts/prepare-desktop-engine.sh`. SHA-256
values were computed from the downloaded official release assets and pinned;
upstream's release API did not provide digest attestations for these assets.

| Artifact | SHA-256 |
| --- | --- |
| stockfish-macos-m1-apple-silicon.tar | `4e23165eb8f353c221ff7ab6716f0a160c3993dadf90d0c0ad982a7ade4091c9` |
| stockfish-macos-x86-64.tar | `067f100a31d3d6f0e45826e6495513c3e0518044e006caa3477993689049d658` |
| nn-1c0000000000.nnue | `1c0000000000a67d629999d932d0c373f7450ce43cd12d0562868f4eaf9ae2ad` |
| nn-37f18f62d772.nnue | `37f18f62d772f3107e1d6aaca3898c130c3c86f2ab63e6555fbbca20635a899d` |

## Distribution and corresponding source

Stockfish is Copyright (C) 2004–2025 The Stockfish developers, derived from
Glaurung 2.1, and licensed under GPL version 3 or later, without warranty.
The engine remains a separate UCI executable; Gambit's own source licenses
are unchanged. Preserve the upstream AUTHORS and Copying.txt files.

The app contains `Contents/MacOS/gambit-stockfish` and, under
`Contents/Resources/engine/`, the license, authors, this notice, and
`stockfish-17.1-source.tar.gz`. The source archive includes upstream C++ source,
Makefile, scripts, README, license, authors, and both matching NNUE files. It
provides source directly with each binary distribution, including updater
archives; no written offer or continued availability of an external URL is
required to obtain this copy. Keep these files when redistributing the app.

To build from the included source, extract it, enter `src`, and use Apple's
Clang with `make -j2 build ARCH=apple-silicon COMP=clang` on Apple Silicon or
`make -j2 build ARCH=x86-64 COMP=clang` on Intel. Run `make clean` before switching
architectures. These are functional rebuild instructions; upstream's release
optimization/toolchain can produce different binary bytes. See the included
Makefile and [upstream build documentation](https://official-stockfish.github.io/docs/stockfish-wiki/Compiling-from-source.html).
The network files are already provided, so rebuilding need not fetch them.

## Development and packaging

```sh
bash scripts/prepare-desktop-engine.sh
cargo test -p gambit-engine
GAMBIT_ENGINE_PATH="$PWD/apps/gambit-desktop/src-tauri/binaries/gambit-stockfish-universal-apple-darwin" \
  cargo test -p gambit-engine --test process packaged_stockfish -- --ignored
cargo run -p gambit-engine --example profile -- \
  apps/gambit-desktop/src-tauri/binaries/gambit-stockfish-universal-apple-darwin
```

`build-desktop-dmg.sh` prepares the engine and merges `tauri.engine.conf.json`
into the release configuration. Tauri treats the engine as an external binary
for signing. The overlay is separate so normal backend tests do not require
downloading engine artifacts. For a native development bundle, prepare the
engine and run Tauri build with `--config src-tauri/tauri.engine.conf.json` from
`apps/gambit-desktop`. No player needs an engine path or Homebrew.

`Gambit.app/Contents/MacOS/gambit-desktop --engine-smoke-test` runs one fixed
position through the adjacent packaged executable, prints raw diagnostic
evidence, and exits without accessing any library or starting the UI. This
explicit diagnostic is the only desktop integration in Phase 0.

## Boundary guarantees

`analyze` accepts a standalone FEN for diagnostics. Game analysis must use
`analyze_game` with a `GamePosition`: its initial FEN and legally replayed UCI
moves are sent together, preserving repetition rather than resetting history
at each decision. Histories are bounded to 1,024 plies. Illegal moves and
malformed input leave the position unchanged; setup FENs cannot supply unknown
earlier history. The six-game profiler records this input as schema 2; its
schema 1 timings remain a separate, FEN-only historical baseline.

The library is synchronous and must be called on a background worker when
integrating the review UI. Each request starts a fresh process with Threads=1,
Hash=16 MiB and MultiPV=1, avoiding retained transposition-table state. Callers
must serialize requests; there is no background queue in this phase. A cloned
cancellation token interrupts handshake, readiness and search. Every return
path sends stop/quit, gives a 250 ms grace period, then kills/reaps the child.
The reader channel holds at most 64 lines of at most 8 KiB; retained PVs have
at most 32 plies and evidence history holds at most 64 primary reports.
Stderr is discarded. A wall-clock watchdog protects against
a hung engine, but the search itself uses `go nodes`, not a time budget.

Scores retain centipawn/mate type and root side-to-move perspective. Explicit
player normalization is available; no player attribution or threshold is
inferred here. Upper/lower score bounds are preserved explicitly, along with
reported search depth, and are never relabeled as exact evidence. Reversing
perspective reverses the bound too. Secondary PVs are ignored. When a node
limit interrupts an aspiration search, the final best move is matched to its
most recent retained report; another move's score is never substituted.
If no matching report exists, the analysis fails. The six-game workload exposed
this case at Kasparov–Deep Blue game 1 after 16.Nh2; its recorded transcript is
covered by a process-level regression test.
Before returning evidence, every retained PV move is replayed legally from the
root and rendered in SAN. Illegal best moves, illegal continuations, and an
engine claiming no move when legal moves exist fail the request. `pv` and
`pv_san` retain matching move order; SAN includes legal disambiguation and
check/mate suffixes. Presentation should hide both until practice allows reveal.
Cancellation and failure return no partial analysis. The application integration
must supply queue cancellation on library switch, sleep and exit before it adds
long-running interactive analysis.

## Profiling and remaining release gates

September 7, 2026, Apple M3, native arm64 official binary, one thread, 16 MiB
hash, fresh process per position, provisional 100,000 nodes:

| Fixed position (see profile example) | Elapsed, including startup |
| --- | --- |
| Initial position | 1,792 ms |
| After 1.e4 e5 2.Nf3 | 374 ms |
| Kiwipete | 360 ms |
| Total | 2,527 ms |

First-launch overhead is included. This sample is not a six-game benchmark,
does not choose a production node budget, and does not establish cross-CPU
bit-for-bit reproducibility. Fixed nodes and a fresh single-thread engine make
repeat measurements possible; callers must still key evidence by engine build
and settings when caching is implemented.

Before declaring Phase 0/release acceptance complete:

- Run the native fixed-position probe and six-game workload on a supported
  Intel Mac and Apple Silicon Mac; select the production node budget afterward.
- Check both architectures in the built DMG on native hardware, along with
  signing/notarization and complete source/license contents.
- Verify functional source rebuilds on both architectures against the shipped
  binaries' behavior. Review distribution obligations when engine/build changes.
- Complete these gates before starting cache schema or threshold tuning.

Rosetta execution, where available, is useful compatibility evidence but does
not replace native Intel performance measurements.

The same probe also passed with the baseline Intel executable under Rosetta
on this M3: 2,746 / 708 / 669 ms (4,125 ms total). All three scores and PVs
matched the native run. `tests/fixtures/stockfish-17.1-startpos.uci` records a
native start-position search; the separate shell fixture is synthetic and
exercises process failures, cancellation, and protocol sequencing.

The included source archive was extracted and rebuilt with Apple Clang using
the Apple Silicon command above. All three scores and PVs matched the official
binary (2,507 ms total, including first launch). The arm64 and Intel release
archives' `src` directories were also compared and are identical.

## Six-game validation

The [six-game workload](https://github.com/diegoglozano/gambit/tree/main/benchmarks/engine) now profiles 235 player
decisions (470 fixed-node searches), reports progress and incomplete failures,
and retains bounded/exact score provenance. It uses public historical games;
representative online review sets still need separate quality testing.

The desktop release workflow downloads the same universal DMG onto native
`macos-14` (arm64) and `macos-15-intel` (x86_64) runners. It checks the packaged
engine, runs the six-game workload, rebuilds the included source, and preserves
machine metadata and timings as artifacts. Both jobs gate publication. These
runner architectures follow [GitHub's runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
Adding the jobs is not evidence that they have passed; their results must be
checked for the exact revision being released.
