# Third-party work

Recorded per the [clean-room charter](docs/specs/2026-09-08-clean-room-charter.md) §5.

## Studied, not vendored

- **gamescope** — BSD-2-Clause. The overlay mechanism (`GAMESCOPE_EXTERNAL_OVERLAY`,
  `GAMESCOPE_NO_FOCUS`) was established by inspecting gamescope's atoms and behaviour.
- **mangoapp / MangoHud** — MIT. The reference implementation of a third-party
  process compositing an overlay inside gamescope.

## Vendored

- **DejaVu Sans Mono** (`assets/DejaVuSansMono.ttf`) — DejaVu Fonts License, a
  permissive Bitstream Vera derivative. Used to rasterise HUD text. Chosen
  because it is monospaced, so a changing number does not reflow the overlay.
  Full licence text: [`assets/DejaVuSansMono.LICENSE`](assets/DejaVuSansMono.LICENSE)
  (copied verbatim from `/usr/share/licenses/ttf-dejavu/LICENSE`, which
  matches the text embedded in the font's own name table).

- **eql-info** (`github.com/amerzel/eql-info`, James Whiteneck) — MIT for its
  source; game data excluded. Studied, not vendored: its `SPELL_FORMAT.md`
  documents the field layout of the EverQuest Legends client's
  `spells_us.txt` and `spells_us_str.txt`, which Spec 2 reads from the user's
  own install. Wisp uses the documented layout, confirmed against the local
  files, and none of that project's parser, database or site data.

## Packaging tooling

Used at build time, not vendored, except where noted:

- **`appimagetool`** 1.9.1 — MIT. Fetched at a pinned SHA-256 into a cache
  directory outside the repository (`packaging/release.sh`); used to build
  the AppImage, never copied into the tree.
- **`AppImage/type2-runtime`** release `20251108` — MIT. **Embedded in the
  AppImages we ship** — the one entry in this list that ends up inside a
  released artifact, so it is recorded as vendored-in-output rather than
  merely used. The runtime's own licence text is at
  `https://raw.githubusercontent.com/AppImage/type2-runtime/main/LICENSE`.
- **`flatpak-cargo-generator.py`**, from `flatpak/flatpak-builder-tools` at
  commit `1fc32195e3e60fe5c97f0af646dec7a99df5962b` — MIT by the file's own
  declaration (`__license__ = "MIT"`). Used to generate
  `packaging/flatpak/cargo-sources.json`, which is committed; the generator
  itself is not.
- **`zsync`** — `LicenseRef-Artistic` per its distribution package. Build-time
  only; nothing of it is in a shipped artifact.

Also recorded here because it changes what this file's reader needs to know:
`smithay-client-toolkit`'s `xkbcommon` feature was dropped for the musl
static build, which removes `xkbcommon` from the dependency tree.
