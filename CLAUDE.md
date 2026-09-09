# CLAUDE.md

Guidance for Claude Code when working in this repo.

## What this is

PrimeWatch is a live memory-inspection and 3D-visualization tool for Metroid Prime 1 (GameCube)
running in the Dolphin emulator. It attaches to a running Dolphin process (or loads a raw memory
dump), interprets Dolphin's emulated RAM through a hand-maintained schema of the game's C++ structs
(`.bs` files under `prime_defs/`), and renders an egui inspector plus a wgpu 3D world view built from
that live memory.

## Layout

| Area | Location | Notes |
|---|---|---|
| Struct schema | `prime_defs/prime1/` | **Authoritative.** Add or edit a `.bs` file rather than hardcoding struct offsets in Rust. |
| Schema compiler | `bstruct/` submodule | Consumed via `bstruct::build_directory(dir)` — no JSON round-trip. |
| Memory pipeline | `src/mem/` | `game_memory.rs` (snapshot + endianness), `dolphin_memory.rs` (live attach), `globals.rs`, utils. |
| Struct runtime | `src/structs/` | `GameStruct` schema model and lookup. |
| Live handles | `src/ctx.rs`, `GameInstance` | `Ctx<'a>` bundles `&GameStructs` / `&GameMemory`; `GameInstance` is the (address, type_name) accessor. |
| Inspector / UI | `src/inspector.rs`, `src/app/`, `src/ui_state.rs` | egui windows, input, scripting (`src/app/scripting/`, rhai). |
| 3D world view | `src/world/`, `src/gl/` | wgpu renderer, BVH, cameras. |
| Reference decomp | `../prime-decomp`, `../metaforce` | Real struct offsets and behavior; trust `prime-decomp` for layout. |

## Build & check

```sh
cargo build
cargo clippy --all-targets
cargo test
cargo fmt
```

`.rustfmt.toml` sets 2-space indent — match it. A linker override (`mold` on Linux, `rust-lld` on
Windows) is checked in under `.cargo/`; if a build fails on a missing linker, that's the cause.
(Note: the file is `.cargo/cargo.toml` — cargo only reads `.cargo/config.toml`, so it's currently
inert; rename it if you want the override to apply.)

No live Dolphin is available in this environment for most work. Use the `.raw` dump path
(`mem1.raw` and friends, 0x1800000 bytes of big-endian emulated RAM) for offline testing.
Memory-access code that needs a live process must be manually verified by the user.

## Conventions (keep consistent across every layer)

- **Big-endian conversion happens once**, in `GameMemory` (`src/mem/game_memory.rs`), via
  `from_be_bytes`. Nothing above that layer re-swaps bytes.
- **Address masking**: game pointers are `0x8...`-prefixed effective addresses. Always mask with
  `& 0x7FFFFFFF` before indexing the snapshot — `GameMemory::address_to_offset` already does this;
  route every read through it.
- **Explicit context, no globals.** Thread `&GameStructs` / `&GameMemory` explicitly, bundled into
  the small `Ctx<'a>` struct passed by reference. Don't reintroduce mutable statics.
- **`GameInstance` is the live handle**: an (address, type_name) pair with member traversal that
  auto-derefs pointer members and resolves inherited members. Its read/index contract (bitfield
  offset/length masking, auto-deref on pointer members) is assumed by every `.bs` file and call site
  downstream — change it carefully.
- Add or edit a `.bs` file rather than hardcoding struct offsets in Rust.
- Prefer `Rc<str>` / `Rc<GameStruct>` over cloning `String` / struct maps on hot paths.
