# Releasing Strata

Nothing here runs by itself except on a version tag, and a tag only creates a **draft** release.

## 1. Check a build without releasing anything

Actions > Release > *Run workflow* on `main`. It runs the tests, builds `strata.exe` on a GitHub runner
(static C runtime, build paths remapped), checks the binary for build-machine paths, builds the per-user MSI,
installs it on the runner, runs it, uninstalls it, and keeps `strata-<ref>-windows-x64.zip`, the `.msi` and
their `.sha256` files as a workflow artifact. No release is created.

## 2. Release

1. Bump `version` in `Cargo.toml` (the MSI takes its version from it), merge.
2. Write `RELEASE_NOTES.md` at the repository root (features, known limits, measurements); without it
   the notes are generated from the merged pull requests.
3. Push the tag: `git tag v0.1.0 && git push origin v0.1.0`. The workflow creates a **draft** release with the zip,
   the MSI and their SHA-256 files.
4. Download the files from the draft, try them (Windows Sandbox is handy), then press *Publish release*.

## 3. winget (optional, public)

winget lists a manifest that points at the public MSI of a published release; it hosts nothing itself.
Manifests are proposed by pull request to <https://github.com/microsoft/winget-pkgs>, which runs an automatic
validation (antivirus scan, silent install) before a moderator merges.

```powershell
winget install Microsoft.WingetCreate
wingetcreate new https://github.com/laurentvv/strata/releases/download/v0.1.0/strata-v0.1.0-windows-x64.msi
```

`wingetcreate` fills the manifest (the files in `installer/winget` show the expected shape, identifier
`LaurentVolff.Strata`), validates it and opens the pull request from your GitHub account. Later versions:
`wingetcreate update LaurentVolff.Strata --version <new> --urls <new msi url> --submit`.
The program is unsigned: SmartScreen shows "unknown publisher" until it is signed or has built up a reputation.
