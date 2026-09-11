# Third-party work

Recorded per the [clean-room charter](docs/specs/2026-09-08-clean-room-charter.md) §5.

## Studied, not vendored

- **gamescope** — BSD-2-Clause. The overlay mechanism (`GAMESCOPE_EXTERNAL_OVERLAY`,
  `GAMESCOPE_NO_FOCUS`) was established by inspecting gamescope's atoms and behaviour.
- **mangoapp / MangoHud** — MIT. The reference implementation of a third-party
  process compositing an overlay inside gamescope.

## Vendored

- **DejaVu Sans Mono, DejaVu Sans, DejaVu Sans Bold**
  (`assets/DejaVuSansMono.ttf`, `assets/DejaVuSans.ttf`,
  `assets/DejaVuSans-Bold.ttf`) — DejaVu Fonts License, a permissive
  Bitstream Vera derivative. Mono rasterises the existing HUD text; Sans and
  Sans Bold are the canvas layer's proportional faces. Chosen because all
  three ship in the same `ttf-dejavu` package with the same licence text.
  Full licence text: [`assets/DejaVu.LICENSE`](assets/DejaVu.LICENSE)
  (copied verbatim from `/usr/share/licenses/ttf-dejavu/LICENSE`, which
  matches the text embedded in each font's own name table).

- **eql-info** (`github.com/amerzel/eql-info`, James Whiteneck) — MIT for its
  source; game data excluded. Studied, not vendored: its `SPELL_FORMAT.md`
  documents the field layout of the EverQuest Legends client's
  `spells_us.txt` and `spells_us_str.txt`, which Spec 2 reads from the user's
  own install. Wisp uses the documented layout, confirmed against the local
  files, and none of that project's parser, database or site data.

## Compiled in

Rust crates from crates.io, statically linked into the shipped binaries.
Recorded here because Spec 5 replaced `wisp-config`'s flat key/value format
with TOML, pulling in a crate family none of the earlier specs' dependency
trees had:

- **`toml`** 0.9.12 — MIT OR Apache-2.0. Parses and writes `wisp-config`'s
  TOML config file.
- **`serde_spanned`** 1.1.1 — MIT OR Apache-2.0. `toml`'s transitive
  dependency, span-tracking wrapper types for error messages.
- **`toml_datetime`** 0.7.5 — MIT OR Apache-2.0. `toml`'s transitive
  dependency, the RFC 3339 datetime type.
- **`toml_parser`** 1.1.3 — MIT OR Apache-2.0. `toml`'s transitive
  dependency, the TOML tokenizer and parser (this crate's line is what the
  brief calls "toml_edit/toml_parser" — the current `toml` major version
  uses `toml_parser`, not `toml_edit`).
- **`toml_writer`** 1.1.2 — MIT OR Apache-2.0. `toml`'s transitive
  dependency, TOML serialization.
- **`winnow`** 0.7.15 and 1.0.4 — MIT. The parser-combinator crate `toml`
  and `toml_parser` each depend on; two versions coexist in the dependency
  graph, both MIT.
- **`indexmap`** 2.14.2 — Apache-2.0 OR MIT. Listed in `Cargo.lock` as a
  dependency of `toml` (an optional, feature-gated ordered-map type `toml`
  offers for preserving key order). Not reachable from any workspace binary:
  `cargo tree --target all -e normal,build,dev -i indexmap` prints nothing,
  because `wisp-config` does not enable the feature that activates it.
  Recorded anyway because it is a new entry in the lock, compiled by
  nothing in this workspace's build.

`cargo tree -p wisp-config -e normal` is the source for this list; `cargo
metadata`'s `license` field for each package is the source for the licences.

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
