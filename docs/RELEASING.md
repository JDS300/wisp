# Releasing Wisp

A release is a tag `v<version>` on `main` where `<version>` is exactly the
workspace version in the root `Cargo.toml`, cut by `packaging/cut-release.sh`.
**No other path publishes.** A build that is not a release never reaches
anyone: Gear Lever updates an AppImage from the update source embedded in it,
which points at this repository's releases, so a CI artifact copied into
`~/AppImages` by hand is overwritten the next time Gear Lever runs. That
happened once, back to v0.1.0, on the day Spec 5 was meant to be tested.

## The two channels

| Channel | Version | Tag | GitHub release | The AppImage's update source |
|---|---|---|---|---|
| live | `0.3.0` | `v0.3.0` | normal, and GitHub's "latest" | `gh-releases-zsync\|JDS300\|wisp\|latest\|Wisp-*-x86_64.AppImage.zsync` |
| beta | `0.3.0-beta.1` | `v0.3.0-beta.1` | marked pre-release, never "latest" | `gh-releases-zsync\|JDS300\|wisp\|latest-pre\|Wisp-*-x86_64.AppImage.zsync` |

The channel is decided by the version string alone: a prerelease suffix makes
it a beta. `packaging/release.sh` reads the version, picks `latest-pre` or
`latest`, and says which on stderr (`release.sh: channel beta (latest-pre)`).
`.github/workflows/release.yml` sets `prerelease:` and `make_latest:` from the
same test on the tag name.

**Use a beta** for anything you want to try over the game before everyone gets
it. **Use live** when a beta has held up, or for a change small enough not to
need one.

## Cutting one

```
packaging/cut-release.sh 0.3.0-beta.1 --dry-run     # see what it would do
packaging/cut-release.sh 0.3.0-beta.1               # do it
```

The script refuses, with one line and exit 2, unless: you are on `main`; the
tree is clean; `main` is at `origin/main` after a fetch; the version is valid
Cargo semver and greater than the current one; and the tag does not exist
locally or on origin. Then it bumps `Cargo.toml`, `Cargo.lock` and the
metainfo's `<releases>` (a beta becomes an AppStream `type="development"`
release), re-reads each file to check the edit landed, builds and tests the
edited tree, and only then commits `Release <version>`, tags it and pushes
both in one `git push origin main v<version>`.

If the build or the tests fail, the version edits are left in the working
tree, uncommitted, and it exits 1. Fix and re-run, or
`git checkout -- Cargo.toml Cargo.lock packaging/io.github.jds300.Wisp.metainfo.xml`.

## Afterwards

1. The release run is green:
   <https://github.com/JDS300/wisp/actions/workflows/release.yml>
2. The release carries four assets: the tarball, the AppImage, its `.zsync`,
   and `SHA256SUMS`. The `.zsync` is not optional — Gear Lever detects an
   update from its header's hash.
3. For a live cut, `gh api repos/JDS300/wisp/releases/latest -q .tag_name`
   names the tag you just made. For a beta cut, it still names the previous
   *live* tag; that is the whole point.
4. The AppImage says which channel it is on:
   `readelf -p .upd_info Wisp-<version>-x86_64.AppImage` prints `latest` or
   `latest-pre`.
5. `./Wisp-<version>-x86_64.AppImage --version` prints `wisp <version>`, and
   Gear Lever shows the same string — it reads `X-AppImage-Version` from the
   desktop entry inside the AppImage, which `release.sh` writes.

## What Gear Lever does, and why `latest-pre` works

Verified in its source on 2026-09-11 (`src/models/GithubUpdater.py`,
`UpdateManagerChecker.py`):

- It reads the `.upd_info` ELF section of the installed AppImage and picks its
  GitHub updater when it finds `gh-releases-zsync|`.
- A release field of `latest` calls GitHub's `releases/latest`, which excludes
  prereleases and drafts. **A live user never sees a beta.**
- A release field of `latest-pre` (or `latest-all`; Gear Lever treats them the
  same) lists all releases and takes the newest non-draft. **A beta tester is
  therefore also moved to the next live release when it is newer** — by
  design: the live release is newer and better, and the next beta is offered
  again because it is newer still. Moving back to the live channel for good is
  simply installing a live build.

`latest-pre` is Gear Lever's word, not AppImageUpdate's; the reference
`AppImageUpdate` tool would not resolve it. Accepted: the live channel is
standard, and a beta build is by definition for someone who has read this
file.

## Never

- Copy an AppImage into `~/AppImages` by hand. Gear Lever will overwrite it.
- Tag by hand, or tag a branch. `release.yml` refuses a tag that does not match
  the workspace version, but nothing stops a hand-made tag on the wrong commit
  from wasting a version number.
- Reuse a version number. Bump it; they are free.
