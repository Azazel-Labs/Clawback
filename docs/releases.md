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
