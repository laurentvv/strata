# Code signing for Strata

**Status: not done.** `strata.exe` and the MSI are unsigned, so Windows SmartScreen shows "unknown publisher" the first
time they are run. This page keeps what was found about the options, so the choice can be made when a release is
published. It was checked on 2026-10-15 from the pages linked below; conditions change, so read them again before
applying to anything.

## What signing does and does not do

- It replaces "unknown publisher" with a named publisher. It does **not** switch SmartScreen off: reputation is built
  per file from clean downloads, and a new build starts again. Since 2024 an EV certificate no longer gives instant
  reputation either (Microsoft's own documentation says so), so paying more for EV only to avoid SmartScreen is not
  justified. See [SmartScreen reputation](https://learn.microsoft.com/windows/apps/package-and-deploy/smartscreen-reputation).
- Warnings are therefore expected for the first releases whatever is chosen; the release notes say so.

## Options

| Option | Cost | Fits Strata? | Notes |
|---|---|---|---|
| **SignPath Foundation** (free certificates for open source) | free | probably | MIT licence, public repository and GitHub Actions build are what they ask for. The publisher shown is **"SignPath Foundation"**, not the maintainer. See below. |
| **Azure Artifact Signing** (formerly Trusted Signing) | not checked | **no, as an individual in France** | Public Trust certificates: organizations in the US, Canada, the EU, the UK and a few other countries; individual developers only in the US and Canada. A company in the EU may qualify. [Quickstart](https://learn.microsoft.com/azure/artifact-signing/quickstart) |
| **OV certificate from a certificate authority** | roughly 25 to 300 EUR/USD a year depending on the authority and on the token or cloud option | possible | Needs a hardware token or the authority's cloud signing. Certum sells an "Open Source" certificate (price unclear on its [shop page](https://shop.certum.eu/code-signing.html)). Reputation still has to build. |
| **Microsoft Store (MSIX)** | not checked | would need a new package format | Store apps are re-signed by Microsoft and carry its reputation. Not looked into beyond that. |

## SignPath Foundation: what they ask

From [signpath.org/terms](https://signpath.org/terms) (read 2026-10-15):

- OSI-approved licence for every component, no commercial dual licensing, no proprietary components, no malware or
  hacking tools, actively maintained.
- **Already released** in the form that will be signed: so the first version has to be published unsigned.
- The project home page and the release page carry a **"Code signing policy"** section that names SignPath.io and
  SignPath Foundation, lists the team roles and members, and states a privacy policy.
- Authors, reviewers and approvers are defined and every signing request is approved by the team (how that works for a
  single maintainer is to be asked); multi-factor authentication for the team on SignPath and on the repository.
- Product name and version are set in the signed files (Strata's executable already carries a product name and a
  version taken from `Cargo.toml`; SignPath asks for them to be enforced, which is to be set up with them).
- The binary is built from the repository by an automated build that SignPath can verify; software must respect
  privacy, announce system changes and be uninstallable (the MSI and the portable mode are).

## Suggested path

1. Publish v0.1.0 unsigned (the release notes already warn about SmartScreen).
2. Apply to SignPath Foundation with the published release; add the "Code signing policy" section to the README and
   to the release text.
3. Once approved, add the signing step to `.github/workflows/release.yml` (their GitHub Actions integration; the
   workflow already builds on a GitHub runner from the tagged source), sign `strata.exe` before the MSI is built, then
   the MSI.
4. Keep the SHA-256 files: they are computed after signing.

Nothing in the repository depends on signing yet.
