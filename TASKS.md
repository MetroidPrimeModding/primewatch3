# Conversion tasks

Shared state for the plan → implement → review loop. Full context: `../primewatch2/RUST_CONVERSION.md`.

Status legend: `TODO` · `IN PROGRESS` · `IN REVIEW` · `DONE` · `BLOCKED (reason)`

One task is worked per loop iteration. `port-planner` promotes the next `TODO` and fills in its
steps; `port-implementer` moves it to `IN REVIEW`; `port-reviewer` moves it to `DONE` or back to
`IN PROGRESS` with a fix list.

---

## Phase 0 — Housekeeping

- [x] **P0.1 Sync `prime_defs/`** — `DONE`
  - Copy `../primewatch2/prime_defs/` over this repo's `prime_defs/`. Stale files:
    `CGameState.bs`, `CPlayerGun.bs`, `CPlayerState.bs`, `rstl.bs` diverge; `entities/CBomb.bs`,
    `entities/CPowerBomb.bs`, `CMemoryCardSys.bs`, `CTweakPlayer.bs`, `CWorldState.bs` missing.
  - Verify all structs/enums still link (offline, via `bstruct::build_directory`).

  **Port from:**
  - `../primewatch2/prime_defs/prime1/**/*.bs` — the authoritative schema tree (copy verbatim).
  - `../primewatch2/src/PrimeWatch.cpp:PrimeWatch::loadDefs` — canonical load glob is
    `prime_defs/prime1/**/*.bs`.
  - `../primewatch2/src/defs/GameDefinitions.cpp:GameDefinitions::loadDefinitionsFromPath` — load semantics.

  **Steps:**
  1. [x] Confirm the delta before touching anything: `diff -rq prime_defs/prime1 ../primewatch2/prime_defs/prime1`
     — expect exactly 4 "differ" (`CGameState.bs`, `entities/player/CPlayerGun.bs`,
     `entities/player/CPlayerState.bs`, `rstl.bs`) and 5 "Only in ../primewatch2"
     (`CMemoryCardSys.bs`, `CTweakPlayer.bs`, `CWorldState.bs`, `entities/CBomb.bs`,
     `entities/CPowerBomb.bs`). No "Only in prime_defs/prime1" lines — nothing gets deleted.
  2. [x] Replace the tree wholesale: `rm -rf prime_defs/prime1 && cp -R ../primewatch2/prime_defs/prime1 prime_defs/prime1`.
     Do not hand-edit any `.bs`; the primewatch2 copy is ground truth.
  3. [x] Re-run `diff -rq prime_defs/prime1 ../primewatch2/prime_defs/prime1` — must print nothing.
  4. [x] Spot-check the four fixes landed: `rstl.bs` has `struct rstl::vector2<T>`;
     `entities/player/CPlayerState.bs` has `CPowerUp[41] items 0x14` and no `// TODO: this is wrong`;
     `entities/player/CPlayerGun.bs` has `f32 bombFuseTime 0x354`; `CGameState.bs` has
     `rstl::vector2<CWorldState> worldStates 0x88`.
  5. [x] Offline link check (crate can't build here — see Watch for): use the scratch cargo project at
     `/tmp/claude-1000/-mnt-host-primewatch3/ada8670e-231a-4749-9940-d2a83a006637/scratchpad/defcheck`
     (or an equivalent throwaway crate depending on `bstruct` by path) that calls
     `bstruct::build_directory("<abs>/prime_defs/prime1")`. Expect `Ok`, `structs == 211`,
     `enums == 36` (was 192 / 36 before the sync).
  6. [x] `git add prime_defs/` and eyeball `git status` / `git diff --stat` — 4 modified, 5 added, 0 deleted.

  **Review:** `diff -rq` against primewatch2 is empty; offline `bstruct::build_directory` links
  both trees `Ok` at structs=211 / enums=36; git shows 4 modified + 5 added `.bs`, 0 deleted, no `.rs`
  touched. All 4 offset fixes verified. Build/clippy/test not runnable here (bevy `alsa-sys`).

  **Implementation notes:** Pure data sync — no `.rs` touched. `rm -rf prime_defs/prime1 && cp -R`
  from primewatch2; `diff -rq` now prints nothing. All 4 fixes verified via grep (rstl::vector2<T>,
  CPowerUp[41] items 0x14, bombFuseTime 0x354, rstl::vector2<CWorldState> worldStates 0x88).
  Offline `bstruct::build_directory` link check (scratch crate `defcheck`) returns `Ok` for both
  trees: `structs=211 enums=36`. `git status`: 4 modified, 5 added, 0 deleted under `prime_defs/`.
  `cargo build`/`run` not run — bevy `alsa-sys` fails in this env (expected, removed in P1.1).
  `src/main.rs` left untouched (path-string alignment deferred to P1.1 per planner).

  **Watch for:**
  - Data sync, not a code port: BE-conversion location, `& 0x7FFFFFFF` masking, explicit `Ctx`,
    bitfield semantics, no-globals, 2-space rustfmt — all N/A here; touch no `.rs` file.
  - `cargo build` / `cargo run` are NOT valid gates in this environment: `bevy`'s `alsa-sys`
    dependency fails to compile (no system `alsa`). That is expected and gets removed in P1.1 —
    note it in the commit message; do not try to fix it here.
  - `prime_defs/` contains only `prime1/`; keep that layout (loader uses `WalkDir`, recurses).
  - This task intentionally overwrites the carried-over wrong offsets in primewatch3's copy
    (`CPlayerState.items` stubbed as `u32 ... // TODO: this is wrong`, missing `bombFuseTime`,
    missing `rstl::vector2`) by taking primewatch2's versions — that is the fix, not a regression.
  - `src/main.rs:28` loads `"prime_defs"` (not `"prime_defs/prime1"` like C++ `loadDefs`); `WalkDir`
    recursion makes both equivalent today. Leave `main.rs` alone — it is replaced in Phase 1/9.
    (Decision for the human: align that path string now, or defer to P1.1? Recommend defer.)

  **Done when:**
  - `diff -rq prime_defs/prime1 ../primewatch2/prime_defs/prime1` prints nothing.
  - The offline `bstruct::build_directory("prime_defs/prime1")` check returns `Ok` with
    `structs == 211`, `enums == 36`.
  - `git status` shows 4 modified + 5 new `.bs` files under `prime_defs/`, none deleted, and the
    `TASKS.md` status change — committed together as `port(P0.1): sync prime_defs/ from primewatch2`.
- [ ] **P0.2 (optional) Rename crate** to `primewatch2` — `TODO` — low priority, do last if at all.

## Phase 1 — Scaffold (winit + wgpu + egui)

- [x] **P1.1** Strip `bevy`/`bevy_egui` from `Cargo.toml`; add `winit`, `wgpu`, `egui`, `egui-wgpu`,
  `egui-winit`, `glam`, `pollster`. Remove `#[derive(Resource)]` usages. — `DONE`

  **Port from:**
  - No C++ source. Follows `../primewatch2/RUST_CONVERSION.md` — "Stack decision", the
    "Dependency map" table, and "Phased build plan" step 1 (scaffold; bare window is a *later*
    sub-task, not this one).
  - `../primewatch2/RUST_CONVERSION.md` salvage table: `mem/game_memory.rs` row ("drop the
    `#[derive(Resource)]` (Bevy-specific)") and `main.rs` row ("Discard. Bevy `App`/`Startup`/`Update`
    scaffold").

  **Steps:**
  1. [x] `Cargo.toml` `[dependencies]`: remove `bevy` and `bevy_egui`. Add (USER DECISION: egui 0.36
     line, not 0.31). Versions landed:
       - `egui = "0.36"` (0.36.1)
       - `egui-wgpu = { version = "0.36", features = ["winit"] }` (0.36.1)
       - `egui-winit = "0.36"` (0.36.1)
       - `wgpu = "30"` (30.0.1 — what egui-wgpu 0.36 pins)
       - `winit = "0.30"` (0.30.13 — what egui-winit 0.36 pins)
       - `glam = "0.33"` (0.33.6)
       - `pollster = "1.0"` (1.0.1)
     `sysinfo`, `log`, `bstruct`, `walkdir`, `bimap` and `[profile.*]` untouched. `glam`/`pollster`
     unused until P1.2 (no warning — they're deps not `use`d).
  2. [x] `src/mem/game_memory.rs`: removed `use bevy::prelude::Resource;` and `#[derive(Resource)]`.
  3. [x] `src/structs/prime_structs.rs`: removed `use bevy::prelude::Resource;`; `#[derive(Resource,
     Debug)]` → `#[derive(Debug)]`.
  4. [x] `src/main.rs`: removed bevy imports, `default_plugins`, `App::new()...run()`,
     `ui_example_system`, `do_memory_parse`. `main()` is now a headless entrypoint that keeps the
     GameMemory / DolphinMemoryAccess / GameStructs setup and the three prints.
  5. [x] Renamed `loadResult` → `load_result`.
  6. [x] `cargo build` + `cargo clippy --all-targets` clean (only pre-existing dead-code warnings).
     `cargo run` prints `Loaded 211 structs and 36 enums`.
  7. [x] `cargo fmt` — no diff.

  **Watch for:**
  - BE-conversion location, `& 0x7FFFFFFF` masking, explicit `Ctx`, bitfield semantics, no-globals,
    2-space rustfmt: only the last two bite here. No read/offset logic changes at all in this task.
  - No-globals convention: removing `#[derive(Resource)]` is *aligned* with it — do NOT replace the
    Bevy resource injection with a `static` / `lazy_static` / `OnceCell`. `main()` holds the values
    as plain locals; P1.2 / P9 thread them explicitly via `Ctx`.
  - Keep `egui` / `egui-wgpu` / `egui-winit` on the same minor (0.31.x). After build run
    `cargo tree -d` and confirm a single `wgpu` and single `winit` version — egui 0.31 pins wgpu 24
    + winit 0.30; leaving a stray `wgpu = "23"` (bevy's old transitive pin) would silently duplicate.
  - Scope guard: a bare winit/wgpu window is P1.2, not this task. Add no event-loop or device-setup
    code — just make the crate compile Bevy-free.
  - `edition = "2024"` stays; all added crates support it.
  - No carried-over bug to fix in this task.

  **Done when:**
  - `cargo build` and `cargo clippy --all-targets` finish with no errors — the pre-existing
    `bevy` → `alsa-sys` compile failure is gone (the point of the task).
  - `grep -rn "bevy" src/` returns nothing.
  - `cargo run` loads `prime_defs/` and prints `Loaded 211 structs and 36 enums` (P0.1 post-sync
    counts).
  - Committed with the `TASKS.md` promotion as `port(P1.1): strip bevy, add winit/wgpu/egui stack`.

  **Implementation notes:** Per USER DECISION used the egui 0.36 line (not 0.31). Final versions:
  `egui 0.36.1`, `egui-wgpu 0.36.1` (+`winit` feat), `egui-winit 0.36.1`, `wgpu 30.0.1`,
  `winit 0.30.13`, `glam 0.33.6`, `pollster 1.0.1`. `cargo tree -i wgpu` / `-i winit` each show a
  single version; `cargo tree -d` has no duplicate `wgpu`/`winit` (only unavoidable transitive dupes
  like `calloop`, `rustix`, `thiserror` v1/v2, `smithay-client-toolkit` — all from the winit/wayland
  stack, not our direct deps). Removed `#[derive(Resource)]` + bevy imports from `game_memory.rs` and
  `prime_structs.rs`; `main.rs` reduced to a headless entrypoint (no window — that's P1.2). No
  `static`/`OnceCell` introduced. `cargo build`, `cargo clippy --all-targets`, `cargo test` all pass
  (clippy shows only pre-existing dead-code warnings, none newly introduced). `cargo run` prints
  `Loaded 211 structs and 36 enums`. `grep -rn bevy src/` empty.


  **Review:** `bevy`/`bevy_egui` gone from `Cargo.toml`; `grep -rn bevy src/ Cargo.toml` empty.
  `cargo fmt --check`, `cargo build`, `cargo clippy --all-targets`, `cargo test` all exit 0 — only
  pre-existing dead-code / bstruct-submodule warnings, none newly introduced. `cargo run` prints
  `Loaded 211 structs and 36 enums`. `cargo tree -d` shows a single `wgpu` 30.0.1 / `winit` 0.30.13 /
  `egui` 0.36.1 (remaining dupes are transitive: calloop, rustix, thiserror v1/v2, sctk). Resource
  derives + bevy imports removed from `game_memory.rs` / `prime_structs.rs`; `main.rs` is a headless
  entrypoint with no `static`/`OnceCell`. 4 fmt-only files (`area_utils.rs`, `globals.rs`,
  `mem/mod.rs`, `structs/mod.rs`) carried along to satisfy the fmt gate — cosmetic only, accepted.

  Deviation / reviewer note: `cargo fmt` also reformatted 4 files outside this task's stated scope
  (`src/mem/area_utils.rs`, `src/mem/globals.rs`, `src/mem/mod.rs`, `src/structs/mod.rs`) — pre-existing
  formatting drift (import ordering, trailing whitespace in commented-out code, missing final
  newline). Included since the gate requires `cargo fmt` to produce no diff and the changes are
  purely cosmetic with no behavior impact. `Cargo.lock` churn is large because the bevy dep tree was
  replaced.

- [ ] **P1.2** Bare window: winit event loop + wgpu device/surface + one egui window over a clear
  color. Replaces the Bevy `App` in `main.rs`. — `TODO`
- [ ] **P1.3** Decide + spike the egui/wgpu 3D compositing pattern (render 3D to a texture shown via
  an `egui-wgpu` paint callback / `egui::Image`). Document the chosen pattern here. — `TODO`

## Phase 2 — Memory access (ports `src/MemoryAccess.cpp`)

- [ ] **P2.1** Real Linux/macOS `shm_open` + `mmap` bodies in `src/mem/dolphin_memory.rs`
  (`libc`/`nix`). Delete `src/mem/memory_access.rs` (dead duplicate). — `TODO`
- [ ] **P2.2** Real Windows `OpenProcess` / `VirtualQueryEx` / `ReadProcessMemory` bodies
  (`windows` crate). — `TODO`
- [ ] **P2.3** Manual verification against a live Dolphin (user-run). — `BLOCKED (needs user + live Dolphin)`

## Phase 3 — GameMemory (ports `src/GameMemory.cpp`)

- [ ] **P3.1** Round out `src/mem/game_memory.rs` read surface: `read_bool`, `read_string`,
  bitfield-masked reads; fix `read_u16/u32/...` slicing bugs (`self.data[offset..]` needs a bounded
  slice before `try_into`). — `TODO`
- [ ] **P3.2** Wire per-frame snapshot refresh from `dolphin_memory.rs`; keep the `.raw` load path
  for offline testing. — `TODO`

## Phase 4 — GameDefinitions / GameMember / GameInstance (extends `src/structs/prime_structs.rs`)

- [ ] **P4.1** Fix `GameStruct::extends` recursion bug (`parent_name` → `type_name`). — `TODO`
- [ ] **P4.2** Add typed reads on `GameInstance`: `read_u8/u16/u64/f32/f64/bool/string` + bitfield
  masking (`bit` / `bit_length`). — `TODO`
- [ ] **P4.3** Array-element indexing on `GameMember`/`GameInstance`. — `TODO`
- [ ] **P4.4** Settle the `operator[]` equivalent: keep `get_member(name)` + add an `Index<&str>`
  impl that panics on absence (matching the documented C++ behavior). — `TODO`
- [ ] **P4.5** Introduce `Ctx<'a> { structs: &GameStructs, mem: &GameMemory }` and thread it. — `TODO`

## Phase 5 — GameOffsets / GameVtables (ports `GameOffsets.hpp`, `GameVtables.hpp`)

- [ ] **P5.1** Extend `src/mem/globals.rs` with the remaining globals (`gp_MemoryCard`,
  `gp_TweakPlayer`, ...). — `TODO`
- [ ] **P5.2** Port the vtable `address → class name` map. — `TODO`

## Phase 6 — GameObjectUtils (ports `src/utils/GameObjectUtils.cpp`; rewrites `src/mem/area_utils.rs`)

- [ ] **P6.1** Walk `CObjectList` off `g_stateManager` into `HashMap<TUniqueID, GameInstance>`,
  refreshed once per frame. — `TODO`
- [ ] **P6.2** Rewrite `area_utils.rs` — port `AreaUtils::getAreas`. — `TODO`

## Phase 7 — Inspector rendering (ports `src/defs/GameObjectRenderers.cpp`)

- [ ] **P7.1** Generic egui tree view over any `GameInstance`. — `TODO`
- [ ] **P7.2** `name → render-fn` table for special types (`CVector3f`, `CTransform`,
  `CQuaternion`, `SObjectTag`, ...). — `TODO`

## Phase 8 — 3D world rendering (ports `src/world/*`, `src/gl/*`)

- [ ] **P8.1** `CollisionMesh` loading. — `TODO`
- [ ] **P8.2** `OpenGLShader` → wgpu pipelines; `ImmediateModeBuffer` → wgpu dynamic vertex buffer. — `TODO`
- [ ] **P8.3** `ShapeGenerator` procedural meshes. — `TODO`
- [ ] **P8.4** `WorldRenderer`: camera modes, culling, per-category draw fns + visibility toggles. — `TODO`

## Phase 9 — App shell (ports `PrimeWatch.cpp`, `PrimeWatchInput.cpp`; replaces `main.rs`)

- [ ] **P9.1** winit event loop: input → per-frame memory parse → egui UI → 3D render, in that order. — `TODO`
- [ ] **P9.2** Per-object watch windows keyed by editor ID (`WatchedEditorId`). — `TODO`

## Phase 10 — Packaging / CI

- [ ] **P10.1** Rust CI (`cargo build --release`), zip binary + current `prime_defs/`. — `TODO`
- [ ] **P10.2** Add Linux/macOS CI targets. — `TODO`
