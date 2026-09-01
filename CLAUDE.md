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
rationale, and phased task breakdown live in **`../primewatch2/RUST_CONVERSION.md`**. Only planning
work (the orchestrator or `port-planner`) needs the full plan; the implementer and reviewer work from
the promoted task entry in **`TASKS.md`**, which is the current task state and the shared loop state.

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

Conversion work runs as a plan → implement → review → **commit** loop over `TASKS.md`:

1. **Plan** — normally done inline by the orchestrator: pick the next unblocked task from `TASKS.md`,
   promote it to `IN PROGRESS`, and fill in its **Steps** / **Port from** / **Watch for** /
   **Done when**. `Port from` must cite exact C++ line ranges (`MemoryAccess.cpp:120-180:symbol`) plus
   a 2-3 line behavioral spec, so the implementer and reviewer read bounded regions, not whole files.
   Spawn the **`port-planner`** subagent only when the next task is genuinely ambiguous or
   under-specified in the plan and needs its own research pass.
2. **`port-implementer`** — implements one task: ports the named C++ code, follows the conventions
   above, gets `cargo clippy --all-targets` + `cargo test` clean.
3. **`port-reviewer`** — checks the implementation against the C++ source of truth and the
   conventions; on an untouched handoff tree it trusts the implementer's pasted command output and
   re-runs only `cargo test`, reporting pass or a fix list.
4. **Archive + commit** — once the reviewer marks the task `DONE` it:
   - moves the task's full entry (Steps, Port from, Watch for, Implementation notes, Review, manual
     checklists) out of `TASKS.md` into `completed_tasks/<task id>.md`, and replaces it in `TASKS.md`
     with a short `DONE` summary — the forward-relevant facts only: what shipped, what APIs/decisions
     now exist, and any deviation or forward-dependency a later task must respect. This keeps
     `TASKS.md` lightweight and scoped to current/future work. `BLOCKED` manual-verification tasks
     stay in `TASKS.md` in full until they clear.
   - commits that task's work (code + the `TASKS.md` summary + the new `completed_tasks/` file) as
     one self-contained commit before starting the next task. Never carry uncommitted work from one
     task into the next — each loop iteration starts from a clean working tree so a bad task is a
     single `git revert`.

Commit message form: `port(P4.2): <what>` — the task ID, then a one-line summary, then a short body
noting the C++ source ported and any deviation. End with the standard Claude Code trailers.

Do the conversion on a dedicated branch (e.g. `rust-conversion`), not `main` — create it before the
first task if it doesn't exist.

Spawn subagents only when asked, and only `port-implementer` / `port-reviewer` by default (plus
`port-planner` for an ambiguous task). One task per loop iteration. Keep `TASKS.md` current — it is
the shared state between iterations.
