# Releasing Clawback

Release tooling is Rust: `cargo xtask`. You need Rust and Git, with push access to
the repository. VS Code provides the same commands under **Tasks: Run Task →
Clawback: Release …**.

## Publish a version

Start on `main`. For example, to release `0.1.0`:

```sh
cargo xtask release prepare 0.1.0
git diff -- Cargo.toml Cargo.lock
git add Cargo.toml Cargo.lock
git commit -m "Release 0.1.0"
git push origin main
cargo xtask release publish 0.1.0
```

If the version already matches, Prepare has nothing to change; skip the empty
commit. Commit and push any other intended changes before Publish.

Prepare updates the shared app/core version, their dependency requirement, and
the lockfile without upgrading third-party dependencies. Publish requires a clean
`main` at the same commit as `origin/main`, creates an annotated `v0.1.0` tag,
and pushes that tag. It never force-pushes or replaces a version tag.

Follow the [Release workflow](https://github.com/Azazel-Labs/Clawback/actions/workflows/release.yml).
It checks the tag against the manifests and lockfile, runs formatting, strict
Clippy and tests on Linux, macOS and Windows, then builds all six platform/CPU
archives. Publication waits for every build to succeed and includes checksums
and automatically generated release notes.

## Stable, previews, and nightly

| Version or trigger | Result | GitHub Latest |
| --- | --- | --- |
| `0.1.0` → `v0.1.0` | Stable versioned release | Promoted to Latest |
| `0.2.0-alpha.1`, `0.2.0-beta.1`, `0.2.0-rc.1` | Versioned prerelease | Unchanged |
| Daily schedule or manual Release workflow | Replaced `nightly` prerelease | Unchanged |

Use the same Prepare/Publish commands for preview versions. A preview's version
must match its tag, including the suffix. Stable promotion is a new version:
prepare and publish `0.2.0` after testing `0.2.0-rc.1`.

Latest is a label on a stable version, not a separate tag. Each newly published
stable release becomes Latest, including an older maintenance version if you
publish one afterward. Change that label in GitHub's release editor if needed.
The permanent download destination for the current stable version is
<https://github.com/Azazel-Labs/Clawback/releases/latest> (available after the first
stable release). Older versions remain on the Releases page.

Nightlies run at 06:17 UTC when there are new commits. To force one, run the
Release workflow manually on **main** and enable **force**.

## Failed releases

If checks or builds fail, no release is published. Rerun a transient failure in
GitHub Actions. If code needs fixing, commit the fix and prepare a new version;
do not move an existing version tag. If the tag push itself fails, the command
prints the exact `git push` command to retry.

You can check a version without publishing anything:

```sh
cargo xtask release validate v0.1.0
```

Builds are currently unsigned; see the [first-run notes](development.md#release-builds).

## WinGet and Homebrew after publication

The Release workflow calls **Distribute stable release** after publishing a stable
release. Publishing a stable release through GitHub also triggers that workflow.
Nightlies and previews are excluded. It generates WinGet manifests and a macOS
Homebrew formula from the published version's URLs and `SHA256SUMS.txt`, and saves
them as a workflow artifact. No Python or PowerShell scripts are required.

Catalog submission is disabled until the following one-time setup is complete:

1. Make the release downloads publicly accessible without authentication. The
   workflow checks all four download URLs anonymously before submitting.
2. Create the public repository **Azazel-Labs/homebrew-tap**, initialized with a
   README so it has a default branch. The workflow manages `Formula/clawback.rb`.
3. Under this repository's **Settings → Secrets and variables → Actions**, add
   secret **HOMEBREW_TAP_TOKEN**: a fine-grained GitHub token with Contents
   read/write access to the tap repository. Authorize organization access if required.
4. Add secret **WINGET_TOKEN** for the GitHub account submitting to
   `microsoft/winget-pkgs`. Follow the
   [WingetCreate token requirements](https://github.com/microsoft/winget-create#github-personal-access-token-classic-permissions).
   It must be able to create a fork, push its branch, and open the submission PR.
   Secrets belong in Actions settings, never in source files.
5. Add Actions **variable** `DISTRIBUTION_ENABLED` with value `true`.

Once enabled, each current **Latest** stable release updates the tap after an
install/version smoke test and submits the WinGet manifests for both Windows
architectures. The same submission handles the first WinGet registration; it
still requires Microsoft's validation/review before users can install it.
Existing WinGet versions and open version PRs are skipped on retries. Homebrew
updates are ordinary commits with no force-pushes.

To distribute an already-published release, use **Actions → Distribute stable
release → Run workflow**, select **main**, and enter its tag, such as `v0.1.0`.
This uses current tooling without moving the release tag or rebuilding binaries.
Only the current Latest stable release is submitted, preventing an old retry
from downgrading the tap. Leave `DISTRIBUTION_ENABLED` unset to generate and
inspect metadata without submitting anything.

After acceptance/publication, users install with:

```sh
winget install --id AzazelLabs.Clawback --exact
brew install Azazel-Labs/tap/clawback
```

Distribution failures do not remove the published GitHub release. Correct the
setup and rerun the distribution workflow. These jobs cover WinGet and our
Homebrew tap; Linux archives remain on GitHub Releases.
