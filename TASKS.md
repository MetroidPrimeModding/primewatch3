# Conversion tasks

Shared state for the plan → implement → review loop. Full context: `../primewatch2/RUST_CONVERSION.md`.

Status legend: `TODO` · `IN PROGRESS` · `IN REVIEW` · `DONE` · `BLOCKED (reason)`

One task is worked per loop iteration. `port-planner` promotes the next `TODO` and fills in its
steps; `port-implementer` moves it to `IN REVIEW`; `port-reviewer` moves it to `DONE` or back to
`IN PROGRESS` with a fix list.

---

## Phase 0 — Housekeeping

- [ ] **P0.1 Sync `prime_defs/`** — `TODO`
  - Copy `../primewatch2/prime_defs/` over this repo's `prime_defs/`. Stale files:
    `CGameState.bs`, `CPlayerGun.bs`, `CPlayerState.bs`, `rstl.bs` diverge; `CBomb.bs`,
    `CPowerBomb.bs`, `CMemoryCardSys.bs`, `CTweakPlayer.bs`, `CWorldState.bs` missing.
  - Verify `cargo run` still loads all structs/enums without link errors.
- [ ] **P0.2 (optional) Rename crate** to `primewatch2` — `TODO` — low priority, do last if at all.

## Phase 1 — Scaffold (winit + wgpu + egui)

- [ ] **P1.1** Strip `bevy`/`bevy_egui` from `Cargo.toml`; add `winit`, `wgpu`, `egui`, `egui-wgpu`,
  `egui-winit`, `glam`, `pollster`. Remove `#[derive(Resource)]` usages. — `TODO`
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
