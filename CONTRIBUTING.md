# Contributing to Strata

Thanks for your interest! Bug reports, ideas and pull requests are welcome.

## Build and test

```bash
cargo run --release -- [PATH]
cargo test
cargo clippy --all-targets -- -D warnings
```

CI runs clippy and the tests on Windows, and the tests on Linux. Please make sure all three commands
pass before opening a pull request.

## Guidelines

- One focused change per pull request, on a `feat/` or `fix/` branch.
- Every user-visible label is written in English and needs its French text in `src/i18n_fr.rs`
  (a test checks this).
- Do not add third-party code, artwork or translation catalogues; everything in the repository must
  be original or under a compatible license.
- Never scan a whole drive from automated tests: use temporary directories.
- Do not attach screenshots or logs that contain personal data (user names, private paths).

## Reporting bugs

Use the issue templates. Include your Windows version, how the scan was started (GUI or `--cli`) and,
if possible, the steps to reproduce.
