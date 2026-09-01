# CLAUDE.md

Guidance for Claude Code when working in this repo.

## What this is

This is the **Rust rewrite** of PrimeWatch — a live memory-inspection and 3D-visualization tool
for Metroid Prime 1 (GameCube) running in the Dolphin emulator. It attaches to a running Dolphin
process (or loads a raw memory dump), interprets Dolphin's emulated RAM through a hand-maintained
schema of the game's C++ structs (`.bs` files), and renders an egui inspector plus a wgpu 3D world
view built from that live memory.

The crate is currently named `primewatch3` for historical reasons; treat "primewatch2" and
"primewatch3" as the same project. Renaming the crate is a low-priority task, not a blocker.

## The conversion

We are porting the C++ app at `../primewatch2` to Rust, in place, in this project. The full plan,
rationale, and phased task breakdown live in **`../primewatch2/RUST_CONVERSION.md`** — read it before
starting any conversion work. Current task state is tracked in **`TASKS.md`** in this repo.

### Sources of truth

| Thing | Location | Notes |
|---|---|---|
| The plan | `../primewatch2/RUST_CONVERSION.md` | Stack decision, salvage assessment, 10-phase build order, open risks. |
| C++ to port from | `../primewatch2/src/**` | Each phase in the plan names the exact C++ file(s) it ports. Port *behavior*, not line-for-line style. |
| C++ architecture notes | `../primewatch2/CLAUDE.md` | Memory pipeline, addressing/endianness gotchas, schema DSL. |
| Struct schema | `../primewatch2/prime_defs/` | **Authoritative.** This repo's `prime_defs/` is stale — copy the primewatch2 copy over before relying on it (see plan's "First housekeeping step"). |
| Native schema compiler | `bstruct/` submodule | Used via `bstruct::build_directory(dir)` — no JSON round-trip. |

## Build & check

```sh
cargo build
cargo clippy --all-targets
cargo test
cargo fmt
```

`.rustfmt.toml` sets 2-space indent — match it. A linker override (`mold` on Linux, `rust-lld` on Windows) is checked in under `.cargo/`; if a build
fails on a missing linker, that's the cause. (Note: the file is `.cargo/cargo.toml` — cargo only reads
`.cargo/config.toml`, so it's currently inert; rename it if you want the override to apply.)

No live Dolphin is available in this environment for most work. Use the `.raw` dump path
(`../primewatch2/mem1.raw`, 0x1800000 bytes of big-endian emulated RAM) for offline testing.
Memory-access code that needs a live process must be manually verified by the user.

## Porting conventions (decided in RUST_CONVERSION.md — keep consistent across every layer)

- **Big-endian conversion happens once**, in `GameMemory` (`src/mem/game_memory.rs`), via
  `from_be_bytes`. Nothing above that layer re-swaps bytes.
- **Address masking**: game pointers are `0x8...`-prefixed effective addresses. Always mask with
  `& 0x7FFFFFFF` before indexing the snapshot — `GameMemory::address_to_offset` already does this;
  route every read through it.
- **Explicit context, no globals.** The C++ side uses ambient global state (`GameMemory::memory`,
  the defs registry). Rust threads `&GameStructs` / `&GameMemory` explicitly. Bundle them into one
  small `Ctx<'a>` struct passed by reference rather than reintroducing mutable statics.
- **Bottom-up phase order.** Don't build a layer before the one under it is working and sanity-checked.
- **`GameInstance` is the live handle** (C++ `GameMember`): an (address, type_name) pair with member
  traversal that auto-derefs pointer members and resolves inherited members. Its read/index contract
  must exactly match the C++ `GameMember::read_*` / `operator[]` semantics — bitfield offset/length
  masking, auto-deref on pointer members — because every `.bs` file and call site downstream assumes it.
- Add or edit a `.bs` file rather than hardcoding struct offsets in Rust.
- Strip Bevy as you go: `bevy` / `bevy_egui` deps, `#[derive(Resource)]`, the `App`/`Startup`/`Update`
  scaffold in `main.rs` all get replaced with winit + wgpu + egui.

### Known carried-over bugs to fix (from the plan)

- `GameStruct::extends` (`src/structs/prime_structs.rs`) recurses with `parent_name` instead of the
  target `type_name` — fix in Phase 4.
- `GameStruct::get_member_by_name` inherited-member lookup should be verified against the same fix.

## The implementation loop

Conversion work runs as a plan → implement → review → **commit** loop over `TASKS.md`, using three
subagents in `.claude/agents/`:

1. **`port-planner`** — picks the next unblocked task from `TASKS.md`, breaks it into concrete steps
   with the exact C++ source references, updates `TASKS.md`.
2. **`port-implementer`** — implements one task: ports the named C++ code, follows the conventions
   above, gets `cargo build` + `cargo clippy` clean.
3. **`port-reviewer`** — checks the implementation against the C++ source of truth and the
   conventions, runs build/clippy/test, reports pass or a fix list.
4. **Commit** — once the reviewer marks the task `DONE`, commit that task's work (code + the
   `TASKS.md` status change) as one self-contained commit before starting the next task. The
   reviewer does this as its last step. Never carry uncommitted work from one task into the next —
   each loop iteration starts from a clean working tree so a bad task is a single `git revert`.

Commit message form: `port(P4.2): <what>` — the task ID, then a one-line summary, then a short body
noting the C++ source ported and any deviation. End with the standard Claude Code trailers.

Do the conversion on a dedicated branch (e.g. `rust-conversion`), not `main` — create it before the
first task if it doesn't exist.

Spawn the subagents only when asked. One task per loop iteration. Keep `TASKS.md` current — it is the
shared state between iterations.
