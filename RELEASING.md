# Releasing Gambit

Gambit releases are built and published by the `Release` GitHub Actions
workflow. The tag version must match the `gambit` package version.

## Release checklist

1. Update all workspace package versions and the matching changelog section.
2. Run the local release checks:

   ```console
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo test --workspace
   cargo dist generate --check
   cargo dist plan --tag=v0.8.0
   oranda build
   mkdocs build --strict
   ```

3. Merge the release-preparation pull request and update local `main`.
4. Create and push one annotated tag from the tested merge commit:

   ```console
   git tag -a v0.8.0 -m "Gambit v0.8.0"
   git push origin v0.8.0
   ```

5. Watch the `Release` workflow through artifact builds and publication.
6. Watch the `Web` workflow rebuild the Oranda install page after `Release`
   completes.
7. Verify `gambit --version` from an installed artifact and run Doctor against
   one plain PGN plus one `.pgn.zst` file.

Do not reuse or move a published version tag. If publication fails after the
GitHub release is visible, diagnose the workflow before creating a new patch
version.
