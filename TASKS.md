# Conversion tasks

Shared state for the plan → implement → review loop. Full context: `../primewatch2/RUST_CONVERSION.md`.

Status legend: `TODO` · `IN PROGRESS` · `IN REVIEW` · `DONE` · `BLOCKED (reason)`

One task is worked per loop iteration. The orchestrator promotes the next `TODO` and fills in its
Steps / Port-from ranges (or spawns `port-planner` if the task is ambiguous); `port-implementer`
moves it to `IN REVIEW`; `port-reviewer` moves it to `DONE` or back to `IN PROGRESS` with a fix
list.

**Archival:** when `port-reviewer` marks a task `DONE`, it moves the task's full entry (Steps, Port
from, Watch for, Implementation notes, Review, manual checklists) to `completed_tasks/<task id>.md`
and leaves behind only a short `DONE` summary here — enough that a future task can see what shipped,
what APIs/decisions now exist, and any forward-dependency or deviation it must respect. This keeps
`TASKS.md` scoped to current and future work. `BLOCKED` manual-verification tasks (e.g. `P2.3`) stay
here in full until they clear.

---

## Phase 0 — Housekeeping

- [x] **P0.1** Sync `prime_defs/` from primewatch2 — `DONE` · full detail: [`completed_tasks/P0.1.md`](completed_tasks/P0.1.md)
  - Replaced `prime_defs/prime1/` wholesale with primewatch2's authoritative copy (4 modified, 5 added `.bs`, 0 deleted; no `.rs` touched). Offline `bstruct::build_directory` links `Ok` at structs=211 / enums=36 (was 192 / 36). Fixed carried-over wrong offsets: `CPlayerState` `CPowerUp[41] items 0x14`, `CPlayerGun` `bombFuseTime 0x354`, `rstl::vector2<T>`, `CGameState` `worldStates 0x88`. Commit `d96d981`.

## Phase 1 — Scaffold (winit + wgpu + egui)

- [x] **P1.1** Strip `bevy`/`bevy_egui`; add winit/wgpu/egui stack — `DONE` · full detail: [`completed_tasks/P1.1.md`](completed_tasks/P1.1.md)
  - USER DECISION: egui **0.36** line. Final deps: `egui 0.36.1`, `egui-wgpu 0.36.1` (+`winit` feat), `egui-winit 0.36.1`, `wgpu 30.0.1`, `winit 0.30.13`, `glam 0.33.6`, `pollster 1.0.1`. Removed `#[derive(Resource)]` + bevy imports from `game_memory.rs` / `prime_structs.rs`; `main.rs` reduced to a headless entrypoint — no `static`/`OnceCell`. `cargo tree -d` shows single `wgpu`/`winit`/`egui`. Commit `ed910c4`.

- [x] **P1.2** Bare winit + wgpu + egui window — `DONE` · full detail: [`completed_tasks/P1.2.md`](completed_tasks/P1.2.md)
  - `src/app.rs`: `App` owns `GameStructs` / `GameMemory` / `DolphinMemoryAccess` as plain fields (+ load-status string) and impls winit `ApplicationHandler`; `AppWindow` (built in `resumed`) holds the wgpu surface/device/queue + `egui::Context` / `egui_winit::State` / `egui_wgpu::Renderer`. One egui window shows the defs load status (or a "NOT LOADED" fallback with the C++ text) over a `wgpu::Color::BLACK` clear. `main.rs` = module decls + `app::run()`. Continuous redraw via `about_to_wait` (`// TODO(P9)` to frame-pace). Numerous wgpu-30 / egui-0.36 API deviations — see archive.
  - **Not run in this env (no display)** — manual checklist in the archive is still pending the human.
  - _Follow-up:_ port the C++ NOT-LOADED "Reload" button (needs `&mut GameStructs` in the egui closure; fold into P9). Commit `3c25868`.

- [x] **P1.3** Spike the egui/wgpu 3D compositing pattern — `DONE` · full detail: [`completed_tasks/P1.3.md`](completed_tasks/P1.3.md)
  - **Decision — pattern B:** 3D renders into an app-owned offscreen `Rgba8Unorm` colour + `Depth32Float` depth target, handed to egui as a user texture (`register_native_texture` once, then `update_egui_texture_from_wgpu_texture` every frame — that call also absorbs resize), drawn as `egui::Image` in a panel; the egui pass still clears the swapchain to black. The world view owns its own depth/camera/clear. **P8 contract:** `SpikeScene::render` = "give me an encoder + a target size, I hand back a `TextureView`" — `WorldRenderer::render` drops into its place.
  - **Gamma note for P8:** the egui composite target is *linear* `Rgba8Unorm` (egui-wgpu hard-requires it), not the surface's sRGB — the real renderer must do its own linear→sRGB.
  - `src/scene.rs` `SpikeScene::{new,resize,render}` (rotating depth-tested indexed cube), owned by `AppWindow`; one-frame lag on panel resize (documented). glam 0.33 deprecates `Mat4::perspective_rh`/`look_at_rh` → used `glam::camera::rh::*`. No `bytemuck` (local `as_bytes` helper). **Not run here.** Commit `7903490`.

## Phase 2 — Memory access (ports `src/MemoryAccess.cpp`)

- [x] **P2.1** Real POSIX `shm_open` + `mmap` Dolphin attach — `DONE` · full detail: [`completed_tasks/P2.1.md`](completed_tasks/P2.1.md)
  - Ported `MemoryAccess.cpp` Linux/macOS `attachToProcess` / `detachFromProcess` / `dolphin_memcpy` into `src/mem/dolphin_memory.rs` under one `#[cfg(any(linux, macos))]` impl (added `libc = "0.2"`). Deleted the dead duplicate `src/mem/memory_access.rs`. `DOLPHIN_SHM_SIZE = 0x2040000` (mmap span) alongside `DOLPHIN_MEMORY_SIZE = 0x1800000` (copy cap). Added `impl Drop` / `impl Default`. Every `unsafe` has a `// SAFETY:` note.
  - Sanctioned deviations from C++: OOB offset → `return false` (not silent read-from-0); log, don't `exit(4)`, on munmap failure; `dolphin_memcpy` bounds the copy by `dest.len()` too (fixes a prior overrun for short `dest`).
  - Windows arms still stubbed (`// P2.2:`). **Live-Dolphin verification is P2.3 (blocked).** Commit `a892fb8`.

- [x] **P2.2** Real Windows `OpenProcess` / `VirtualQueryEx` Dolphin attach — `DONE` · full detail: [`completed_tasks/P2.2.md`](completed_tasks/P2.2.md)
  - Ported `MemoryAccess.cpp` `getEmuRAMAddressStart` / `attachToProcess` / `detachFromProcess` / `dolphin_memcpy` into the `#[cfg(target_os = "windows")]` arms + a private `get_emu_ram_address_start`. `windows = "0.62"` target-gated under `[target.'cfg(windows)'.dependencies]` — never enters the Linux/macOS build. Region scan: `VirtualQueryEx` loop, first `RegionSize == 0x2000000 && Type == MEM_MAPPED` that `QueryWorkingSetEx` marks valid; `Valid` is bit 0 of the `Flags` union (`windows 0.62` has no `.Valid()` accessor). `get_dolphin_pids` widened to lowercased-stem `starts_with("dolphin")` (covers `Dolphin.exe`). Gate: `cargo check --target x86_64-pc-windows-msvc`.
  - Sanctioned deviations: `MEM2Present` dropped (dead); `OpenProcess` failure → `return false`; `dest.len()` clamp; OOB → `false`.
  - **P3.2 forward-dependency:** the Windows `dolphin_memcpy` stays `&self` and does **not** self-heal a stale `emu_ram_address_start` (C++ zeroes it on `ERROR_PARTIAL_COPY`). P3.2 MUST re-attach (or `detach_from_process`) per frame or a Windows session gets stuck returning `false` after the game closes/reloads.
  - **Phase 2 is code-complete;** only P2.3 (live verification) remains, blocked. Commit `a7c9a54`.

- [ ] **P2.3** Manual verification against a live Dolphin (user-run). — `BLOCKED (needs user + live Dolphin)`

  **POSIX (P2.1 — Linux/macOS + live Dolphin):**
  - [ ] With MP1 running in Dolphin: `get_dolphin_pids()` returns its pid; `attach_to_process(pid)` returns `true`.
  - [ ] `dolphin_memcpy(&mut buf, 0, 0x1800000)` fills a `0x1800000`-byte buffer; `&buf[0..6] == b"GM8E01"`
    (matches `../primewatch2/mem1.raw` first bytes) and a live field (e.g. `g_stateManager` chain) reads sanely.
  - [ ] Dropping / re-attaching does not leak (check `/proc/<our-pid>/maps` shrinks after `detach_from_process`).

  **Windows (P2.2 — Windows box + live Dolphin):**
  - [ ] `get_dolphin_pids()` returns Dolphin's pid; `attach_to_process(pid)` returns `true` and logs a "Found ram start" line.
  - [ ] `dolphin_memcpy(&mut buf, 0, 0x1800000)` fills the buffer; `&buf[0..6] == b"GM8E01"`.
  - [ ] Closing the game mid-session: the next `dolphin_memcpy` returns `false` and a later re-attach
    recovers without a leak (Task Manager handle count stable).
  - [ ] Linux/macOS behaviour is unchanged (POSIX path untouched).

## Phase 3 — GameMemory (ports `src/GameMemory.cpp`)

- [x] **P3.1** Round out the `GameMemory` read surface — `DONE` · full detail: [`completed_tasks/P3.1.md`](completed_tasks/P3.1.md)
  - Fixed-width reads (`read_u16/u32/u64/i16/i32/i64/f32/f64`) now route through a private `read_bytes<const N>` (masks via `address_to_offset`, bounded `.get(offset..offset+N)`) — fixes the old `self.data[offset..]` bug that misread every non-terminal address and panicked past EOF. Added `read_i8..i64`, `read_bool`, `read_string` (255 cap, NUL/OOB terminator, raw `byte as char`), `extract_bits` (ports `getBits`, shift-UB guarded) + `read_u{8,16,32,64}_bits`. `from_be_bytes` stays the sole BE conversion point.
  - **OOB reads return `None`** (deviation from C++ `getRealPtr` clamp-to-0). **P4.2 `GameInstance` owns**: default substitution when a read is `None`, and the `bit`/`bit_length` `Option<i64>`→`u32` unwrap at its boundary.
  - Tests read `../primewatch2/mem1.raw` (skip-if-absent). Commit `5004acd`.

- [x] **P3.2** Heap-allocate the `GameMemory` snapshot, then wire its per-frame refresh from
  `dolphin_memory.rs` into `App`; keep the `.raw` load path for offline testing. — `DONE`

  **Port from:**
  - `../primewatch2/src/GameMemory.cpp:14-27` — `memory` (the `array<char, DOLPHIN_MEMORY_SIZE>`
    snapshot), `updateFromDolphin`, `loadFromPath`. `updateFromDolphin`: *if* `getAttachedPid() > 0`,
    `dolphin_memcpy(memory.data(), 0, memory.size())` — a no-op that just leaves the buffer intact
    when detached. `loadFromPath`: open the file, `read` up to `DOLPHIN_MEMORY_SIZE` bytes into the
    snapshot, print the count (a short dump is not an error).
  - `../primewatch2/src/PrimeWatch.cpp:56-70` — startup order in `initAndCreateWindow`: `loadDefs()`,
    then `GameMemory::updateFromDolphin()` (called once here purely to touch/init the buffer — a
    no-op now that it is always allocated; skip this call), then `pids = getDolphinPids()`, then
    `if (pids.size() == 1) attachToProcess(pids[0])` — auto-attach only when exactly one Dolphin is
    running.
  - `../primewatch2/src/PrimeWatch.cpp:99-103` — `initGlAndImgui` loads `./mem1.raw` via
    `loadFromPath` iff `std::filesystem::exists("./mem1.raw")`. This runs *before* `loadDefs()` /
    the attach; a later live memcpy simply overwrites it.
  - `../primewatch2/src/PrimeWatch.cpp:483-488` — `doMemoryParse`: `if (isLoaded()) { updateFromDolphin(); ... }`
    — the per-frame refresh, gated on defs being loaded, runs at the top of each frame before any
    parse/render.
  - Current Rust: `src/mem/game_memory.rs` (`GameMemory`, `new`, `load_from_file`), `src/app.rs`
    (`App::new`, `window_event` RedrawRequested, `about_to_wait`),
    `src/mem/dolphin_memory.rs:296` (`get_attached_pid`), `:305` (`dolphin_memcpy`), `:51` /
    `:156` (`get_dolphin_pids` / `attach_to_process`).

  **Steps:**
  1. [x] Heap-allocate the snapshot (folds in the P3.1 stack-overflow TODO). Change
     `GameMemory::data` to `Box<[u8; SNAPSHOT_LEN]>`. `new()`:
     `vec![0u8; N].into_boxed_slice().try_into().expect(..)` (safe, no stack array). `Box<[u8; N]>`
     derefs to `[u8; N]`, so `self.data.get(..)`, `self.data.len()`, `self.data[..n]` in the existing
     reads and tests keep working untouched.
  2. [x] `load_from_file`: read straight into the boxed array via a `reader.read(&mut self.data[filled..])`
     loop (handles `ErrorKind::Interrupted`), tolerating a short read like C++ `loadFromPath`; log the
     byte count (`Read {filled:#x} bytes`). Dropped the `vec![0; N] … try_into().unwrap()` stack
     materialisation.
  3. [x] Added `GameMemory::update_from_dolphin(&mut self, dolphin: &DolphinMemoryAccess)` porting
     `updateFromDolphin`: `if dolphin.get_attached_pid() > 0 { dolphin.dolphin_memcpy(&mut self.data[..], 0, DOLPHIN_MEMORY_SIZE); }`.
     Reconciled the duplicate `DOLPHIN_MEMORY_SIZE` — `game_memory.rs` now `use`s the `usize`
     constant from `dolphin_memory` (was a local `u32` copy), aliased as `SNAPSHOT_LEN` for the
     array-type spelling.
  4. [x] `App::new` (`src/app.rs`): after the defs-load block — (a) if `Path::new("./mem1.raw").exists()`
     → `mem.load_from_file("./mem1.raw")`, log ok/err, don't abort; (b) `dolphin.get_dolphin_pids()`,
     if `pids.len() == 1` → `dolphin.attach_to_process(pids[0].as_u32() as i32)`, log the result;
     `> 1` logs "not auto-attaching". `mem` / `dolphin` are now `mut` locals; dropped their
     `#[allow(dead_code)]`.
  5. [x] Per-frame refresh: in `window_event`'s `WindowEvent::RedrawRequested` arm, before
     `window.render(...)`, `if self.defs_loaded { self.mem.update_from_dolphin(&self.dolphin); }`
     (ports `doMemoryParse`'s `isLoaded()` gate). `// TODO(P9.1):` left noting the entities parse and
     the real input→parse→ui→render ordering land here.
  6. [x] Gates: `cargo fmt --check` clean, `cargo clippy --all-targets` clean (28 warnings, down
     from the 32 baseline — the `#[allow(dead_code)]` removals and now-used `new`/`load_from_file`
     shrank the dead-code cascade; no new warning kinds), `cargo test` 4 passed / 0 failed.

  **Watch for:** (as promoted — all respected)
  - Stack overflow was the point of step 1: `vec!`-based heap alloc, never `Box::new([0u8; N])`.
  - No globals: `update_from_dolphin` takes `&DolphinMemoryAccess`; `App` owns both as plain fields.
  - BE conversion untouched; no read logic changed.
  - Detached is not an error — `get_attached_pid() > 0` guard, buffer left as-is (keeps the `.raw`).
  - `.raw` path parity — hardcoded `./mem1.raw` literal in `App::new`.
  - Scope guard honoured: no `GameInstance` / `GameObjectUtils` / event-loop restructure.

  **Done when:** (all met)
  - `GameMemory` holds `Box<[u8; SNAPSHOT_LEN]>`; no bare stack array constructed by value.
  - `cargo test` passes; `reads_against_mem1` still runs (not skipped) against
    `../primewatch2/mem1.raw` — and now exercises the rewritten `load_from_file` directly.
  - `cargo clippy --all-targets` + `cargo fmt --check` clean, no new warning kinds.
  - `App::new` auto-attaches on exactly one Dolphin PID and loads `./mem1.raw` when present; the
    RedrawRequested path calls `update_from_dolphin` once per frame behind the `defs_loaded` gate.
  - `grep -n "OnceCell\|lazy_static\|static mut" src/mem/game_memory.rs src/app.rs` → nothing.

  **Implementation notes (P3.2):**
  - `GameMemory::data: Box<[u8; SNAPSHOT_LEN]>` where `SNAPSHOT_LEN: usize = dolphin_memory::DOLPHIN_MEMORY_SIZE`
    (the old local `const DOLPHIN_MEMORY_SIZE: u32` is gone — single source of truth now the
    `dolphin_memory` `usize` constant). `new()` = `vec![0u8; SNAPSHOT_LEN].into_boxed_slice().try_into().expect(..)`
    — a safe heap allocation with no intermediate stack array. All reads/tests untouched (`Box<[u8;N]>`
    derefs to `[u8;N]`).
  - `load_from_file` now loops `reader.read(&mut self.data[filled..])` until `Ok(0)`, continuing on
    `ErrorKind::Interrupted`, propagating other errors. A file shorter than the snapshot leaves the
    tail bytes as they were (C++ `ifstream::read` parity); a longer file is truncated at
    `SNAPSHOT_LEN`. Prints `Read {n:#x} bytes`.
  - `update_from_dolphin(&mut self, &DolphinMemoryAccess)` — `get_attached_pid() > 0` gate, then
    `dolphin_memcpy(&mut self.data[..], 0, DOLPHIN_MEMORY_SIZE)`. No-op while detached.
  - `App::new`: `.raw` load + auto-attach ordered *after* the defs-load block (the two are
    field-independent, so the C++ ordering — `.raw` in `initGlAndImgui` before `loadDefs` — does not
    matter). `pids[0].as_u32() as i32` for the sysinfo `Pid` → `attach_to_process(i32)` conversion.
    The C++ init-time `updateFromDolphin()` "touch the buffer" call is dropped (the buffer is always
    allocated now).
  - Per-frame: one `self.mem.update_from_dolphin(&self.dolphin)` in the `RedrawRequested` arm behind
    `self.defs_loaded`. Disjoint-field borrow (`self.mem` / `self.dolphin` vs. the `window` local
    that reborrows `self.window`) — compiles clean under NLL.
  - Test helper `blank()` is now just `GameMemory::new()` (the P3.1 `Box::new_zeroed().assume_init()`
    unsafe would be UB now that `GameMemory` contains a `Box` pointer — a zeroed pointer is null).
    `load_mem1()` now calls `mem.load_from_file(&path)` instead of hand-rolling `fs::read` +
    `copy_from_slice`.
  - Not run in this env (no display) — the winit/wgpu window path is still only compile-checked;
    `App::new`'s attach logic runs `get_dolphin_pids()` (returns `[]` here, no Dolphin) but is not
    exercised against a live process. Live verification rolls into P2.3 / P9.

  **Review (P3.2):** Reviewed inline (project `port-implementer`/`port-reviewer` subagents are not
  registered in this environment). `cargo fmt --check` exit 0; `cargo clippy --all-targets` 28
  warnings, all pre-existing dead-code in `src/structs/**` / `src/mem/**` / `bstruct/` (count down
  from 32 — no new warning kinds, none pointing at `app.rs`, the two `game_memory.rs` hits are the
  pre-existing dead `trait MemoryAccess` and the dead-code cascade on private read helpers);
  `cargo test` 4 passed / 0 failed, `reads_against_mem1` ran (not skipped). Behaviour checked
  arm-by-arm against C++: `updateFromDolphin` `getAttachedPid() > 0` guard → `> 0` (not `>= 0`);
  `dolphin_memcpy(_, 0, size)` → same args; `loadFromPath` short-read tolerance preserved
  (loop breaks on `Ok(0)`, tail untouched); `initAndCreateWindow` "attach iff exactly one PID"
  preserved; `./mem1.raw` literal preserved. No globals introduced (`grep` clean). BE conversion
  untouched. Heap-allocation requirement met (`vec!` → boxed slice, no stack array; `Box::new([0;N])`
  avoided). Scope stayed within the task (no P4/P6/P9 work). Deviations, all sanctioned/minor:
  `SNAPSHOT_LEN` alias for readability over bare `DOLPHIN_MEMORY_SIZE`; `.raw`+attach ordered after
  defs-load (field-independent); init-time `updateFromDolphin()` touch-call dropped as dead.

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
!