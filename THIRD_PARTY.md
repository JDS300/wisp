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

Spec 6 added a system-tray item, which pulls in `ksni` and the `zbus` D-Bus
stack. Built with `default-features = false, features = ["blocking",
"async-io"]`: no `tokio`, and `zbus` needs no C library, so the static musl
build is unaffected.

- **`ksni`** 0.3.6 — **Unlicense** (public-domain dedication; the crate's own
  `[package] license` field, checked with `cargo metadata`). Spec 6 §4.2 calls
  it MIT; the crate says Unlicense, and the crate is right. A
  StatusNotifierItem and its `com.canonical.dbusmenu` menu, in pure Rust.
- **`zbus`** 5.19.0 — MIT. The D-Bus session-bus connection the tray
  registers over.
- **`zbus_macros`** 5.19.0 — MIT. `zbus`'s derive macros.
- **`zbus_names`** 4.3.4 — MIT. Well-known and unique D-Bus name types.
- **`zvariant`** 5.15.0, **`zvariant_derive`** 5.15.0, **`zvariant_utils`**
  4.2.0 — MIT. The D-Bus wire type system `zbus` serialises over.
- **`zcheapstr`** 1.1.0 — MIT. `zvariant`'s small-string optimisation.
- **`endi`** 1.1.1 — MIT. Endian-aware (de)serialisation for `zvariant`.
- **`memoffset`** 0.9.1 — MIT. Field-offset macro used by `zvariant`.
- **`uds_windows`** 1.2.1 — MIT. Unix-domain-socket shim for Windows targets;
  unreachable on the musl/Linux binaries this project ships.
- **`tracing-attributes`** 0.1.31 — MIT. `#[instrument]` for `zbus`'s
  tracing calls; `tracing` itself was already in the tree before Spec 6.
- **`async-io`** 2.6.0, **`async-executor`** 1.14.0, **`async-lock`** 3.4.2,
  **`async-task`** 4.7.1, **`async-channel`** 2.5.0, **`async-broadcast`**
  0.7.2, **`async-process`** 2.5.0, **`async-signal`** 0.2.14,
  **`async-recursion`** 1.1.1, **`async-trait`** 0.1.92, **`blocking`**
  1.7.0, **`piper`** 0.2.5, **`parking`** 2.2.1, **`event-listener`** 5.4.2,
  **`event-listener-strategy`** 0.5.4, **`atomic-waker`** 1.1.2,
  **`futures-channel`** 0.3.34, **`futures-core`** 0.3.34, **`futures-io`**
  0.3.34, **`futures-lite`** 2.6.1, **`futures-macro`** 0.3.34,
  **`futures-task`** 0.3.34, **`futures-util`** 0.3.34, **`ordered-stream`**
  0.2.0, **`task-local`** 0.1.1, **`fastrand`** 2.5.0,
  **`signal-hook-registry`** 1.4.8 — all Apache-2.0 OR MIT (or the MIT OR
  Apache-2.0 ordering). The `smol`-family async executor `zbus`'s
  `async-io` feature runs on; `blocking` alone would not compile without one.
- **`autocfg`** 1.5.1, **`bumpalo`** 3.20.3, **`enumflags2`** 0.7.12,
  **`enumflags2_derive`** 0.7.12, **`getrandom`** 0.4.3, **`hex`** 0.4.3,
  **`once_cell`** 1.21.4, **`pastey`** 0.2.3, **`proc-macro-crate`** 3.5.0,
  **`rustversion`** 1.0.23, **`serde_repr`** 0.1.21, **`syn`** 2.0.119 (a
  second version alongside the one already in the tree), **`tempfile`**
  3.27.0, **`toml_datetime`** 1.1.1+spec-1.1.0 and **`toml_edit`**
  0.25.15+spec-1.1.0 (both new; a second `toml_datetime` version alongside
  `wisp-config`'s own), **`uuid`** 1.26.1 — all Apache-2.0 OR MIT / MIT OR
  Apache-2.0. Transitive build-time and macro dependencies of the `ksni`/
  `zbus` tree.
- **`r-efi`** 6.0.0 — **MIT OR Apache-2.0 OR LGPL-2.1-or-later**. A
  `getrandom` dependency on UEFI targets only; unreachable on the binaries
  this project ships, recorded because it is new to `Cargo.lock`.
- **`js-sys`** 0.3.105, **`wasm-bindgen`** 0.2.128, **`wasm-bindgen-macro`**
  0.2.128, **`wasm-bindgen-macro-support`** 0.2.128, **`wasm-bindgen-shared`**
  0.2.128 — MIT OR Apache-2.0. `getrandom`'s `wasm32` support; unreachable on
  the binaries this project ships, recorded because they are new to
  `Cargo.lock`.

`git diff 017ec26 -- Cargo.lock | grep '^+name'` is the source for what is
new; `cargo metadata --format-version 1 --locked` is the source for every
licence above.

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
