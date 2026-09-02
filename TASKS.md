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

- [x] **P3.2** Heap-allocate `GameMemory` + wire per-frame Dolphin refresh into `App` — `DONE` · full detail: [`completed_tasks/P3.2.md`](completed_tasks/P3.2.md)
  - `GameMemory::data` is now `Box<[u8; SNAPSHOT_LEN]>` (heap, via `vec!` → boxed slice — fixes the latent ~24 MiB main-stack overflow flagged in P3.1; the old local `const DOLPHIN_MEMORY_SIZE: u32` is gone, `game_memory.rs` `use`s the `usize` one from `dolphin_memory`). `load_from_file` loops `read` and tolerates a short/long file (C++ `loadFromPath` parity).
  - New `GameMemory::update_from_dolphin(&mut self, &DolphinMemoryAccess)` — `get_attached_pid() > 0` gate then `dolphin_memcpy(&mut self.data[..], 0, DOLPHIN_MEMORY_SIZE)`; no-op while detached (keeps any `.raw` data). `App::new` loads `./mem1.raw` when present and auto-attaches iff exactly one Dolphin PID; `window_event` `RedrawRequested` calls `update_from_dolphin` once per frame behind the `defs_loaded` gate.
  - **P4.2 / P6.1 note:** the per-frame hook and the `defs_loaded` gate now exist in `app.rs::window_event` (`// TODO(P9.1)`) — slot the `GameObjectUtils` entities parse there. **Not run** (no display) — attach logic is compile-checked only; live verification is P2.3 / P9. Commit `b500fff`.

## Phase 4 — GameDefinitions / GameMember / GameInstance (extends `src/structs/prime_structs.rs`)

- [x] **P4.1** Fix `GameStruct::extends` recursion bug (`parent_name` → `type_name`) — `DONE` · full detail: [`completed_tasks/P4.1.md`](completed_tasks/P4.1.md)
  - `GameStruct::extends` (`src/structs/prime_structs.rs`) now recurses with the original `type_name`, not the intermediate parent's name — transitive `extends` (`C : B : A`) works. Matches C++ `GameStruct::extendsClass`.
  - `GameStruct::get_member_by_name` confirmed already correct (local map first, then parents with unchanged name; no cycle guard — schema is a DAG). No change made.
  - Added `#[cfg(test)] mod tests` in `prime_structs.rs`: hand-built in-memory `GameStructs` (no `.bs`/`mem1.raw`) covering transitive extends, negatives, inherited member lookup, local override.
  - Both carried-over bugs from CLAUDE.md "Known carried-over bugs" are now resolved.
- [x] **P4.2** Typed reads on `GameInstance` + bitfield masking — `DONE` · full detail: [`completed_tasks/P4.2.md`](completed_tasks/P4.2.md)
  - `GameInstance` (`src/structs/prime_structs.rs`) gains `bit: Option<i64>` / `bit_length: Option<i64>` fields (both `None` from `new`, so `globals.rs` roots unchanged), a `with_bitfield(addr, type_name, bit, bit_length)` ctor, and typed reads `read_u8/u16/u32/u64/f32/f64/bool/string` (`&self, mem: &GameMemory) -> Option<T>`). Integer reads route through `GameMemory::read_u*_bits` with the `Option<i64>`→`u32` clamp (`unwrap_or(0).max(0)`) done at the boundary; `f32/f64/string` take no masking; `read_bool` = `read_u8 != 0`. Ports C++ `GameMember::read_*`.
  - `get_member` now carries `member.bit` / `member.bit_length` into the returned instance (pointer auto-deref path unchanged).
  - **Deviation (orchestrator-sanctioned, overrides the P3.1 forward-note):** reads return `Option` with NO default substitution; defaulting deferred to P7 render callsites (`.unwrap_or_default()`), so reads compose with `?` and the inspector can distinguish "unreadable" from "zero".
  - No `read_i*` variants (not in the C++ `GameMember` surface). Pre-existing `collapsible_if` lints in P4.1 `get_member_by_name`/`extends` remain — optional cleanup for a later P4 task.
- [x] **P4.3** Array-element indexing on `GameInstance` — `DONE` · full detail: [`completed_tasks/P4.3.md`](completed_tasks/P4.3.md)
  - `src/structs/prime_structs.rs`: free `pub fn primitive_size(type_name: &str) -> u32` (exact port of C++ `GameDefinitions::primitiveSize`; `u64`/`i64` deliberately fall through to the `_ => 4` default). `GameInstance` gains `array_length: Option<i64>` + private `with_member(addr, &GameMember)` ctor; `get_member` now carries `bit`/`bit_length`/`array_length` through it. `new` / `with_bitfield` signatures unchanged (globals.rs roots untouched).
  - `GameInstance::element_size(&self, &GameStructs) -> u32` = struct `size` (`.max(0) as u32`) else `primitive_size`. `GameInstance::element(&self, &GameStructs, index: u32) -> GameInstance` = fresh instance at `address.wrapping_add(index.wrapping_mul(element_size))`, same `type_name`, all of `bit`/`bit_length`/`array_length` cleared. No bounds check on `index` — P4.4/P6/P7 own clamp/panic policy against `array_length`.
  - `primitive_size` is unused until P6/P7 wire it (dead-code warning, left unsuppressed to match the rest of the not-yet-wired defs layer).
- [x] **P4.4** `operator[]` equivalent on `GameInstance` — `DONE` · full detail: [`completed_tasks/P4.4.md`](completed_tasks/P4.4.md)
  - `GameInstance::member(&self, &GameStructs, &GameMemory, &str) -> GameInstance` in `src/structs/prime_structs.rs`: a thin `get_member(...).unwrap_or_else(|| panic!("Unknown member {type_name}.{name}"))` — the panicking C++ `GameMember::operator[]` analogue. `get_member` stays the fallible `Option` form (unchanged).
  - **Decision (recorded): no `std::ops::Index` impl.** Member resolution needs `&GameMemory` and returns an owned `GameInstance`, fitting neither `Index::index`'s arg list nor its `&Output` return. `member` is the panicking primitive instead.
  - **P4.5 forward-dependency:** the ergonomic `x["a"]["b"]` chain is deferred to a `Ctx`-based helper built on `member`; a `// P4.5:` hook comment marks the spot.

- [x] **P4.5** Introduce `Ctx<'a>` and thread it through the live-handle layer — `DONE` · full detail: [`completed_tasks/P4.5.md`](completed_tasks/P4.5.md)
  - New `src/ctx.rs`: `#[derive(Clone, Copy)] pub struct Ctx<'a> { pub structs: &GameStructs, pub mem: &GameMemory }` + `Ctx::new`; `mod ctx;` in `main.rs`. Imports only `game_memory` + `prime_structs`.
  - **Layer split (decision):** the `GameInstance` live methods (`get_type`, `get_member`, `member`, `element_size`, `element`, `read_u8/u16/u32/u64/f32/f64/bool/string`) now take `&Ctx`; the pure defs layer (`GameStruct::{get_member_by_name,extends}`, `GameMember::get_type`, `GameEnum::*`, `GameStructs::*`) and the `GameInstance` ctors (`new`/`with_bitfield`/`with_member`) stay `&GameStructs` / context-free. `globals.rs` roots unchanged.
  - Signatures-only, no behavior change. `a.member(ctx,"b").member(ctx,"c").read_u32(ctx)` chains on one `Copy` arg — no helper/macro. `Ctx` is unconstructed in the bin until P6/P7 wire call sites (expected dead-code warning).
  - Completes Phase 4.

## Phase 5 — GameOffsets / GameVtables (ports `GameOffsets.hpp`, `GameVtables.hpp`)

- [x] **P5.1** Extend `src/mem/globals.rs` with the remaining globals (`gp_MemoryCard`,
  `gp_TweakPlayer`). — `DONE` · full detail: [`completed_tasks/P5.1.md`](completed_tasks/P5.1.md)
  - `src/mem/globals.rs`: `get_state_manager()` / `get_main()` unchanged (context-free, `GameInstance`).
    New `get_memory_card(&Ctx)` / `get_tweak_player(&Ctx)` → `Option<GameInstance>`: read the `u32`
    pointer at the fixed address (`0x805A8C44` / `0x805A8CD8`), hand back a handle at that address
    (`CMemoryCardSys` / `CTweakPlayer`). Ports the `GameOffsets.hpp` pointer globals + the
    `GameObjectRenderers.cpp:34-42` deref.
  - Deviation (sanctioned, matches P3.1/P4.2): the deref is fallible — OOB/unreadable → `None`; a
    zero pointer still derefs to address 0. **P7 render callsites own** the `.unwrap_or`/skip policy.

- [x] **P5.2** Port the vtable `address → class name` map. — `DONE` · full detail: [`completed_tasks/P5.2.md`](completed_tasks/P5.2.md)
  - New `src/mem/vtables.rs` (`pub mod vtables;` in `mem/mod.rs`): `MP1_VTABLES:
    LazyLock<HashMap<u32, &'static str>>` — all 142 `GameVtables.cpp` entries verbatim (incl. the
    `0x803d9ce0 → "0x803d9e30"` alias) — plus `vtable_class_name(u32) -> Option<&'static str>`
    (the C++ `.count()?[]` point lookup). Both `pub`; P9's "unknown vtables" UI can iterate the map.
  - **P6.1 forward-note:** C++ retypes an object only when `MP1_VTABLES.count(v) && structByName(name)`
    — the `CObjectList` walk must still confirm the mapped name is a known `.bs` struct before
    retyping the `GameInstance`.

Phase 5 complete.

## Phase 6 — GameObjectUtils (ports `src/utils/GameObjectUtils.cpp`; rewrites `src/mem/area_utils.rs`)

- [x] **P6.1** Walk `CObjectList` off `g_stateManager` into `HashMap<TUniqueID, GameInstance>`, refreshed once per frame — `DONE` · full detail: [`completed_tasks/P6.1.md`](completed_tasks/P6.1.md)
  - New `src/mem/game_object_utils.rs` (`pub mod` in `mem/mod.rs`): `pub type TUniqueID = u16`, `get_all_objects(&Ctx) -> HashMap<TUniqueID, GameInstance>` (walks the intrusive `SObjectListEntry` slot list off `g_stateManager["allObjects"]`, retypes each `entity` by vtable when `vtable_class_name` maps it AND the name is a real `.bs` struct), `get_object_by_entity_id(&Ctx, u16) -> Option<GameInstance>` (`eid & 0x3FF` slot lookup, no retype — for P8 `WorldRenderer` camera).
  - `App` (`src/app.rs`) gains `objects: HashMap<TUniqueID, GameInstance>`, refreshed once per frame in the `RedrawRequested` arm right after `update_from_dolphin`, inside `if self.defs_loaded`, via `Ctx::new(&self.structs, &self.mem)`. Field is `#[allow(dead_code)]` until P7 consumes it.
  - Deviations from C++ (per P4.2/P5.1 `Option`-not-total convention): a mid-walk `None` on any structural/value read stops the walk and returns what was gathered (C++ reads are total, OOB→0; never triggers on a valid snapshot). `get_object_by_entity_id` returns `Option` rather than a always-valid handle.
  - `get_object_by_entity_id` is dead code until P8 wires it (expected warning). `area_utils.rs` still carries pre-existing unused-import warnings — P6.2 rewrites it.

- [x] **P6.2** Rewrite `area_utils.rs` — port `AreaUtils::getAreas` — `DONE` · full detail: [`completed_tasks/P6.2.md`](completed_tasks/P6.2.md)
  - `src/mem/area_utils.rs` rewritten end-to-end: `pub fn get_areas(ctx: &Ctx) -> Vec<GameInstance>` walks `g_stateManager`→`world`→`areas` (`rstl::vector<rstl::autoptr<CGameArea>>`), reads `areas["end"]` as the element count, strides `first` by the monomorphized `rstl::autoptr<CGameArea>` size (0x8), and collects each `["value"]` as a `CGameArea` handle in area-index order.
  - Deviations from C++ (sanctioned): no `name` field on `GameInstance` — the `Vec` index is the label (P7 formats it); `const AREA_CAP: u32 = 1024` bounds the loop (C++ has no failsafe on `end`); `Option` reads bail early with what was gathered instead of fabricating 0.
  - Not yet wired — consumer is P8 `WorldRenderer` (`get_areas` / `AREA_CAP` are expected dead code until then, no `#[allow]`, no speculative `app.rs` call).
  - Phase 6 complete.

## Phase 7 — Inspector rendering (ports `src/defs/GameObjectRenderers.cpp`)

- [x] **P7.1** Generic egui tree view over any `GameInstance` — `DONE` · full detail: [`completed_tasks/P7.1.md`](completed_tasks/P7.1.md)
  - `src/inspector.rs`: `Inspector { exact_values }` (+ `new`/`Default`) with `render(&self, ui, &Ctx, name, &GameInstance, add_tree)` — the recursive egui walk ported from the generic half of `GameObjectRenderers.cpp` (array → primitive → special-hook → `rstl::vector` → enum/struct). Pure `pub` helpers `format_primitive` / `format_enum` / `hover_text` (+ `c_hex_i64` sign-magnitude hex) match the C++ `fmt` strings and are unit-tested.
  - `pub const SPECIAL_TYPES: &[&str]` (`CVector3f` / `CQuaternion` / `CTransform` / `CMatrix4f` / `SObjectTag`) + a `TODO(P7.2)` fall-through in `render` — P7.2 fills in the per-type renderers behind that hook.
  - `GameInstance` gains `pub pointer: bool` (`prime_structs.rs`): `new` / `with_bitfield` / `element` leave it `false`, only `with_member` sets it from `member.pointer`. Used for the `*name` prefix, `address == 0` → "null", and `u8*` → C-string.
  - Deviations (all sanctioned): `render` drops the top-level `derefPointer` branch (instances arrive already-deref'd); `CollapsingHeader::id_salt((name, address))` not address-only; `*`-prefix applied to pointer struct/enum labels too; `ARRAY_CAP = 4096` on array loops; struct body iterates `members_by_offset` (no active bitfields in the schema, so no same-offset collision today — revisit `GameStruct` member ordering if `.bs` bitfields are re-enabled). No call site yet (Phase 9).

- [x] **P7.2** `name → render-fn` table for special types — `DONE` · full detail: [`completed_tasks/P7.2.md`](completed_tasks/P7.2.md)
  - `src/inspector.rs` special-renderer surface: pure `format_vec3` / `format_quat` / `format_matrix_row` (C++ `{:.8}`/`{:.3}`/`{:.2}` specifiers, literal `(c*4 + r)*4` cell arithmetic, trailing `", "` kept) + `impl Inspector` egui methods `render_vec3` / `render_quat` / `render_transform` (cols 3) / `render_matrix4f` (cols 4) / `render_object_tag`, dispatched from `Inspector::render` behind the `SPECIAL_TYPES` guard (precedence: primitive → special → `rstl::vector<` → enum/struct).
  - `src/mem/game_object_utils.rs`: `pub fn four_cc_to_string(u32) -> String` (MSB-first, raw `char::from(u8)`, no sanitizing) and `pub fn object_tag_to_string(&Ctx, &GameInstance) -> String` (`"{id:08x}.{fourCC}"`, members read as u32 with `.unwrap_or(0)`) — P8 `WorldRenderer` reuses `object_tag_to_string`.
  - Deviations (P7.1 precedent, sanctioned): no click-to-copy `clip` hex-bits variants (label text is copied); CollapsingHeader id is `(name, address)` salt not `name###name offset`; address math uses `wrapping_add`.
  - No call site yet (Phase 9). Phase 7 complete.

## Phase 8 — 3D world rendering (ports `src/world/*`, `src/gl/*`)

- [x] **P8.1** `CollisionMesh` loading — `DONE` · full detail: [`completed_tasks/P8.1.md`](completed_tasks/P8.1.md)
  - New `src/world/` module tree: `mod.rs` (`#[repr(C)] pub struct Vert { pos/color/normal/barycentric }`,
    ports `gl/OpenGLMesh.hpp` `Vert`) + `collision_mesh.rs`. `mod world;` in `main.rs`.
  - `ECollisionMaterial(pub u32)` bitflag newtype (33 consts, `REDUNDANT_EDGE`/`FLIPPED_TRI` both `0x2000000`)
    + `contains` (C++ `!!(a & b)`). `CollisionMesh` struct (`raw_*` arrays + `min`/`max`/`materials`/`verts`).
  - `load_mesh(ctx, area) -> Option<CollisionMesh>` ports `WorldRenderer::loadMesh`: walks
    `area->postConstructed->collision["value"]` (`*CAreaOctTree`), copies material/vert/edge/poly arrays
    from game memory off the auto-deref'd pointer `.address`es, records the area AABB, runs `build_vertices`.
    Preserves the C++ quirk: per-vertex materials read from the `polyEdges` pointer, not `vertMats`.
  - `CollisionMesh::build_vertices` ports `initGlMesh`: 3-edge→index resolution, `FLIPPED_TRI` i1/i3 swap,
    normal, verbatim colour ladder (incl. dead `|| n.z > 0.85`); fills `verts: Vec<Vert>` (no GPU).
  - Deviations (sanctioned): memory-derived indices use checked `.get(..).unwrap_or_default()` not C++
    unchecked `operator[]`; `?`-bail on structural misses, `.unwrap_or` on bulk reads; 50000 count cap.
  - **Forward:** all symbols are dead code until P8.4 wires `load_mesh` into the `mesh_by_mrea` cache /
    `updateAreas`; P8.2 adds the `wgpu::VertexBufferLayout` for `Vert` and the GPU upload.
- [x] **P8.2** `OpenGLShader` → wgpu pipelines; `ImmediateModeBuffer` → wgpu dynamic vertex buffer — `DONE` · full detail: [`completed_tasks/P8.2.md`](completed_tasks/P8.2.md)
  - New `src/gl/` module tree (`mod gl;` in `main.rs`): `Vert` moved here from `src/world/mod.rs` (+ `Vert::LAYOUT` wgpu vertex layout, stride 52), `WORLD_COLOR_FORMAT` (`Rgba8Unorm`) / `WORLD_DEPTH_FORMAT` (`Depth32Float`) consts, `pub(crate) as_bytes`, `Topology {Lines,Triangles}`. `src/world/collision_mesh.rs` now `use crate::gl::Vert;`.
  - `gl::mesh::DynamicMesh` (grow-on-demand `VERTEX|COPY_DST` buffer, `new`/`upload`/`draw`; non-indexed). `gl::shader`: `WorldUniforms` (`#[repr(C)]`, 288 B, `from_matrices` fills CPU `normal_matrix` = inverse-transpose of model — sanctioned deviation, WGSL has no `inverse()`), `WORLD_SHADER_WGSL`, `WorldPipelines` (4 pipelines: mesh/line × opaque/translucent; `front_face: Cw`, `cull_mode: None`). `gl::immediate::ImmediateModeBuffer` (CPU-only: accumulates `Vec<Vert>`, `tri_verts()`/`line_verts()` accessors; P8.4 owns the GPU upload).
  - **Sanctioned deviation:** `linear_to_srgb` applied to every shader output (NEW vs C++) to satisfy the P1.3 linear→sRGB contract for the linear `Rgba8Unorm` egui composite target — isolated in one WGSL helper, flag-commented. **P8.4 must verify compositing does not double-encode** and revisit there if so.
  - **P8.4 forward-deps:** `cull_mode: None` on all 4 pipelines — P8.4 owns `CullType` BACK/FRONT/NONE → variant choice (marked `// P8.4:`); MSAA still single-sample; caller normalizes `light_dir` before `set_uniforms`; P8.4 unifies `scene.rs`'s private `COLOR_FORMAT`/`DEPTH_FORMAT` with the `gl` consts. All `gl` symbols dead until P8.4 (expected dead-code warnings, no `#[allow]`).
- [x] **P8.3** `ShapeGenerator` procedural meshes — `DONE` · full detail: [`completed_tasks/P8.3.md`](completed_tasks/P8.3.md)
  - New `src/gl/shapes.rs` (`pub mod shapes;` in `src/gl/mod.rs`): six CPU-only procedural vertex
    generators returning `Vec<Vert>`, ports `ShapeGenerator.cpp` whole file —
    `generate_cube(min: Vec3, max: Vec3, color: Vec4)`,
    `generate_cube_from_center(center: Vec3, size: Vec3, color: Vec4)`,
    `generate_cube_lines(min: Vec3, max: Vec3, color: Vec4)`,
    `generate_sphere(center: Vec3, radius: f32, color: Vec4)`,
    `generate_truncated_sphere(center: Vec3, radius: f32, bottom_distance: f32, color: Vec4)`,
    `generate_camera_line_segments(perspective: Mat4, transform: Mat4, center_line_length: f32)`.
  - Deviations: truncated-sphere apex is `center + Vec3::new(0,0,bottom_latitude_z_dist)` — verbatim
    from `ShapeGenerator.cpp:252`, IS center-offset (the P8.3 planning note that said otherwise was
    wrong). `invert_helper` returns `Vec3` not C++ `vec4` (safe: w==1 after perspective divide).
    `emit_sphere_band` factors the shared quad loop (C++ open-codes it twice).
  - All symbols dead until P8.4 wires them into `WorldRenderer` (expected dead-code warnings, no `#[allow]`).

### P8.4 — WorldRenderer

- [x] **P8.4.1** Port `MathUtils` + `GameInstance::extends_class` — `DONE` · full detail: [`completed_tasks/P8.4.1.md`](completed_tasks/P8.4.1.md)
  - New `src/mem/math_utils.rs` (`pub mod math_utils;` in `src/mem/mod.rs`): free fns `read_as_vec3` / `read_as_quat` / `read_as_matrix4f` / `read_as_transform`, each `(ctx: &Ctx, member: &GameInstance) -> Option<glam>` — port of `MathUtils.cpp`. Quat is `(x,y,z,w)` order; matrix readers resolve `member["matrix"]` / `member["m0"]` `.address` then read per-cell f32s at `base + (r + c*4)*4` and feed them in C++ arg order to `Mat4::from_cols_array` (both column-major → identical byte layout). `read_as_transform` hardcodes col0-2 4th elem `0.0`, col3 `w` `1.0`.
  - `GameInstance::extends_class(&self, ctx: &Ctx, class_name: &str) -> bool` in `src/structs/prime_structs.rs` — exact port of C++ `GameMember::extendsClass`: identity check on own type, else delegate to `GameStruct::extends` (P4.1-fixed recursion); unknown type matches identity-only.
  - **Deviation (sanctioned):** all readers are fallible — missing sub-member or OOB address → `None`, no `unwrap_or_default`/panic. P8.4.2+ call sites own the defaulting policy. The 4 readers are dead code until P8.4.2 wires them.

- [x] **P8.4.2** `WorldRenderer` skeleton + camera modes + collision-mesh GPU cache — `DONE` · full detail: [`completed_tasks/P8.4.2.md`](completed_tasks/P8.4.2.md)
  - Shipped `src/world/renderer.rs`: `WorldRenderer::{new,resize,color_view,update,render}` (drop-in for the deleted `SpikeScene` — same offscreen colour/depth target + egui user-texture contract), `WorldInput` (ports `PrimeWatchInput` minus `capturedMouse`; `app.rs` passes `WorldInput::default()` until P9.1), enums `CullType`/`CameraMode`/`OrbitPlayerCameraOrigin`, `GameCamera`. Pure testable helpers `compute_camera(&CameraParams)`, `quat_from_euler(Vec3)`, `reconcile_area(...)`.
  - `src/gl/shader.rs`: `WorldPipelines` holds the 2 mesh pipelines in all 3 `CULL_MODES` (`[None,Back,Front]`); select via `WorldPipelines::mesh(translucent: bool, cull: Option<wgpu::Face>)`. Lines never culled; opaque+translucent immediate-buffer tris always `Back`; only the `mesh_by_mrea` collision draw honours `self.culling`. `bind_group_layout` field removed. `src/scene.rs` + `mod scene;` gone.
  - Deviations a later phase must respect: `fov` is passed to `perspective()` **unconverted** (verbatim `glm::perspective` port — `45` is likely a latent C++ radians/degrees bug; decision still open, flagged in the manual checklist). `cam_eye` = `cam_view.inverse().w_axis.truncate()` (true world eye) replaces `glm::decompose` — P8.4.5 must re-derive `camPointing`/`camViewport` itself. Player/camera reads keep the last good value on a `None`.
  - Forward-deps for P8.4.3: `update` gains the `objects` map + `highlighted` set; per-class draw fns push into `render_buff` / `translucent_render_buff` (opaque then translucent). Manual checklist (needs display / live Dolphin) still pending a human.

- [x] **P8.4.3** `renderEntities` dispatch + player/trigger/dock/actor/physicsActor draw fns — `DONE` · full detail: [`completed_tasks/P8.4.3.md`](completed_tasks/P8.4.3.md)
  - `src/world/renderer.rs`: ported `render_entities` (full `extends_class` dispatch chain, verbatim C++ order `CCollisionActor`->`CAi`->`CPhysicsActor`->`CActor`; `#[allow(clippy::collapsible_if)]` to keep a class-matched-but-flag-off entity from falling through to a base-class branch), `draw_player` (buffer select by `color.w < 0.99`; speed indicator always on `render_buff`), `draw_trigger` / `draw_dock` / `draw_physics_actor` / `draw_actor`. Seven P8.4.4 draw fns (`draw_projectile`/`_bomb`/`_power_bomb`/`_chozo_ghost`/`_pickup`/`_collision_actor`/`_ai`) are `&mut self` no-op stubs already wired into the chain.
  - New `pub(crate)` pure helpers (unit-tested): `trigger_render_flags` (`detect_projectiles` fans to the 7-bit mask), `trigger_color`, `is_degenerate_bbox`, `physics_actor_bbox` (collisionPrimitive->baseBoundingBox->renderBounds, last one **not** `pos`-offset), `player_speed_color`; chained-read helpers `walk_member` / `read_vec3_at`.
  - `WorldRenderer` now has `player: PlayerGhost` (loose `player_pos`/`velocity`/`orientation`/`is_morphed` folded in), `player_ghosts: [PlayerGhost; 5]`, `trigger_render_config: TriggerRenderConfig`, `actor_render_config: ActorRenderConfig` (plain-`bool` ports of the C++ bitfield structs with matching `Default`). `WorldRenderer::update` signature gained `objects: &HashMap<TUniqueID, GameInstance>` + `highlighted: &HashSet<u16>`; `app.rs` passes `&self.objects` + an empty set (`// TODO(P9.2)`).
  - Deviations (sanctioned): `draw_physics_actor` reads all three bbox sources eagerly and skips the actor on any `None` (C++ reads lazily; equivalent on a valid snapshot). Render order is entities-then-player, both appended at the end of `update` after the camera-line/ghost-cube block (plan-directed; render-equivalent for depth-tested CPU accumulation). The P8.4.4 stubs and specialized geometry are still TODO.

- [x] **P8.4.4** Projectile/bomb/powerBomb/ai/pickup/chozoGhost/collisionActor draw fns — `DONE` · full detail: [`completed_tasks/P8.4.4.md`](completed_tasks/P8.4.4.md)
  - `src/world/renderer.rs`: the seven P8.4.3 dispatch stubs now render real geometry (`WorldRenderer.cpp:775-1023`, no text/screen-space overlays — those are P8.4.5). `draw_ai`/`draw_pickup` reduce to `draw_physics_actor`; `draw_bomb` shadows its highlight param and recomputes via ball proximity; projectile cube is world-space (`Mat4::IDENTITY`), bomb/power-bomb spheres are `Vec3::ZERO` + `set_transform(transform)`.
  - `draw_chozo_ghost` signature gained `objects: &HashMap<TUniqueID, GameInstance>` (threaded through the one `render_entities` call site) — resolves the cover point by slot id `coverPoint & 0x3FF`.
  - New `pub(crate)` pure helpers + unit tests: `bomb_fuse_frames`, `bomb_proximity_highlight`, `projectile_world_pos`, `projectile_world_vel`. Private helper `read_vec3_member` alongside `walk_member`/`read_vec3_at`.
  - Deviations (sanctioned): dropped dead C++ reads (projectile `transform` @821, collision-actor `pos` @977); `CGameProjectile`/`CProjectileWeapon` schema lives in `entities/CWeapon.bs` (no `CGameProjectile.bs`). P8.4.5 still owns all HP/item/fuse-frame text overlays.

- [x] **P8.4.5** Screen-space text overlays + `renderImGui` status windows + `EItemType` enum — `DONE` · full detail: [`completed_tasks/P8.4.5.md`](completed_tasks/P8.4.5.md)
  - Shipped: new `src/defs/` module (`mod defs;`) — `defs::item_types` with `EItemType` (`#[repr(i32)]`, C++ discriminants, `from_raw(u32)` → `Invalid` on unknown) + `item_type_to_name`. `mem::game_object_utils::get_all_loading_datas(&Ctx) -> Vec<GameInstance>` walks `g_main["globalObjects"]["gameResFactory"]["loadList"]`.
  - `WorldRenderer` gained `cam_viewport: [f32;4]` (pixel-space `[0,0,w,h]`, set in `new`/`resize`/`update`), `pub text_overlays: Vec<TextOverlay>` + `clear_text_overlays()` / `add_text_overlay()`, `pub(crate) fn project(pos,view,proj,viewport) -> Vec3` (glm::project port), private `screenspace_pos_for_actor` / `_physics_actor` (Y-flipped), and `pub fn render_status_windows(&self, &Ctx, &mut egui::Ui)` (WorldStatus area/loading grid + PlayerStatus pos/vel/look). `draw_bomb`/`draw_ai`/`draw_pickup` now queue overlays.
  - Call site wired: `app.rs::render` takes `Option<&Ctx>` and hosts `render_status_windows` behind the `defs_loaded` gate.
  - Forward-deps: overlay `screen_pos` is the bare projected point — P9.1's overlay painter owns glyph-metric centering / line-height (`OVERLAY_LINE_HEIGHT = 14.0` is a nominal stand-in). All reads are `Option` and bail rather than C++ total-read-0.

- [x] **P8.4.6** Visibility-toggle UI: Culling/Camera/Triggers/Actors menus + Camera Controls window — `DONE` · full detail: [`completed_tasks/P8.4.6.md`](completed_tasks/P8.4.6.md)
  - `src/world/renderer.rs`: new `pub(crate)` free fns `render_menu_bar(...)` / `render_camera_controls_ui(...)` (plain `&mut` field refs, headless-testable) with thin `WorldRenderer::render_menu` / `render_camera_controls` forwarders. Added `pub manual_camera_speed: f32` (1.0, ports `WorldRenderer.hpp:91`) and `pub show_exact_camera_controls: bool` (false, app-shell state parked here). Ports `PrimeWatch::doMainMenu:383-464` + `doFrame:322-336`.
  - Culling menu keeps the verbatim C++ label/value skew ("Show Front"→`CullType::Back`, "Show Back"→`CullType::Front`). Camera Controls Yaw/Pitch display degrees, write back radians only on `.changed()`; `yaw_deg = deg % 360.0` (matches C++ `fmod` sign).
  - **Deviation:** `use_collision_impulses` checkbox label is the corrected spelling `"useCollisionImpulses"` (C++ has `useCollisionImpluses` sic). **Deviation:** egui 0.36 has no context-level panel API (`TopBottomPanel` gone) — `src/app.rs::render` mounts the menu bar as a top `egui::Area` + `egui::Frame::menu` + `egui::MenuBar::new().ui(...)`, behind `defs_loaded`; `// P9.1:` markers left for the Attach + Tools menus.
  - P9.1 owns: Attach/Tools menus; a `.open()` close binding on the Camera Controls window; shell layout so the `menu_bar` / `world-status-host` Areas (both `fixed_pos (0,0)`) don't overlap.

## Phase 9 — App shell (ports `PrimeWatch.cpp`, `PrimeWatchInput.cpp`; replaces `main.rs`)

- [x] **P9.1** winit event loop: input → memory parse → egui UI → 3D render — `DONE` · full detail: [`completed_tasks/P9.1.md`](completed_tasks/P9.1.md)
  - `src/app.rs` near-total rewrite: `InputState` + `App::accumulate_input`/`device_event` accumulate winit events; pure `InputState::plan(wants_kb, wants_mouse, camera_mode) -> InputPlan` ports `PrimeWatch::processInput` (sticky mouse capture, look, wheel/arrow/PageUp-Down camera deltas, Shift/Ctrl+1-5 ghosts, Detached WASD/QE). `App::redraw` = C++ `mainLoop` order (input → `update_from_dolphin` → `get_all_objects` → `world.update` → `AppWindow::render`).
  - `AppWindow::render` takes `&mut FrameState<'a>` (game/UI state split-borrowed out of `App`). Menu bar = Attach (PID list / detach / rfd `.raw` file picker) + P8.4.6 render-config menus + Tools (Reload Definitions / Raw Data View / Raw Demo View placeholder / exact-values); menu clicks deferred as `MenuAction` applied after `queue.present`. Also: `render_raw_data_view` hex table, per-frame text-overlay painter over the World image, P7 `Inspector` "globals" window, NOT-LOADED "Reload" button.
  - `src/world/renderer.rs` +`record_player_ghost` / `clear_player_ghost` / `move_detached_camera`. `Cargo.toml` +`rfd = "0.15"` (heavy zbus/ashpd tree; synchronous picker — revisit before P10).
  - **P9.2 must respect:** `FrameState` is the seam for the object-table / watch windows; `App.objects` is walked every frame but dropped after `world.update` — thread it into `FrameState`. `highlighted` set fed to `world.update` is currently empty (`// TODO(P9.2)`). Non-blocking gaps: Camera Controls window has no titlebar-X `.open()` bind (menu toggle only); scroll-zoom dead while hovering the World window; shell layout rough. All display/live-Dolphin manual checks still pending the human (see archive).

- [x] **P9.2** Per-object watch windows + Objects table/filter — `DONE` · full detail: [`completed_tasks/P9.2.md`](completed_tasks/P9.2.md)
  - New `src/object_filter.rs`: `ObjectFilter { raw: String }` — `passes(&str) -> bool` (comma-split, `-` negation, empty = pass-all) + `ui(&mut egui::Ui)`. **Deviation:** case-sensitive and negatives always win (`"foo,-bar"` rejects `"foo bar"`) — matches the task prose, not literal ImGui's in-order short-circuit.
  - `src/app.rs`: `struct WatchedEditorId { eid, last_known_uid, type_name }`; `App`/`FrameState` gain `editor_ids_to_watch` / `show_active_in_table_only` (true) / `table_hovered_uid` (0xFFFF) / `object_filter` / `unknown_vtables: BTreeSet<u32>` (session-persistent, grow-only). Free fn `render_objects_window(...)` ports `PrimeWatch.cpp::drawObjectsWindow:502-704` (count, vtable histogram, unknowns, "List of types", filter, uid-sorted entity table, per-`WatchedEditorId` watch-window loop with `Inspector::render(add_tree=false)`), mounted behind the `Some(ctx)` gate next to "globals".
  - `redraw` highlight set is now `{table_hovered_uid unless 0xFFFF} ∪ {watch.last_known_uid ∀ watch}` fed to `WorldRenderer::update` (C++ `doFrame:264-273`); one-frame lag vs C++ (egui pass runs after `world.update`) is sanctioned and documented. `// TODO(P9.2)` gone.
  - Manual checklist (display + live Dolphin) pending human — see archive. After this, `doImGui` is fully ported.

## Phase 10 — Packaging / CI

- [ ] **P10.1** Rust CI (`cargo build --release`), zip binary + current `prime_defs/`. — `TODO`
- [ ] **P10.2** Add Linux/macOS CI targets. — `TODO`
!
