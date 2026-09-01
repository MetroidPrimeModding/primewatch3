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

- [x] **P1.2** Bare window: winit event loop + wgpu device/surface + one egui window over a clear
  color. Replaces the Bevy `App` in `main.rs`. — `DONE`

  **Port from:**
  - `../primewatch2/src/PrimeWatch.cpp:PrimeWatch::initAndCreateWindow` — window creation (1200x800,
    title "Prime Watch 2"), GL/context init, framebuffer-resize callback registration, `loadDefs()`
    call, then `worldRenderer.init()`. Port only the window + surface + egui-context + defs-load parts;
    everything game-specific (memory attach, world renderer) is later phases.
  - `../primewatch2/src/PrimeWatch.cpp:PrimeWatch::initGlAndImgui` — ImGui context/style setup
    (`CreateContext`, `StyleColorsDark`, glfw+opengl3 backends, default font, initial viewport). Rust
    equivalent: `egui::Context` + `egui_winit::State` + `egui_wgpu::Renderer`.
  - `../primewatch2/src/PrimeWatch.cpp:PrimeWatch::mainLoop` — the `while (!windowShouldClose)` loop:
    `processInput()` → `doFrame()` → swap → poll. In winit 0.30 this becomes the `ApplicationHandler`
    callbacks; only the redraw half is in scope for P1.2.
  - `../primewatch2/src/PrimeWatch.cpp:PrimeWatch::doFrame` (lines 235-278) — frame structure:
    egui `NewFrame` → build UI → `glClearColor(0,0,0,1)` + clear → (world render, later) → egui
    `Render`/`RenderDrawData`. Port the clear color `(0.0, 0.0, 0.0, 1.0)` and the "defs not loaded"
    fallback window (lines 244-257) as the single egui window this task shows: if `GameStructs` loaded
    OK show `"Loaded N structs and M enums"`, else show the error text.
  - `../primewatch2/src/PrimeWatch.cpp:framebuffer_size_cb` (search `framebuffer_size_cb` /
    `updateWindowSize`, ~line 485-492) — resize handler: update viewport/aspect. Rust equivalent:
    reconfigure the wgpu surface on `WindowEvent::Resized`.

  **Steps:**
  1. [x] New module `src/app.rs` (declare `mod app;` in `main.rs`). Put everything for this task there;
     do not split into submodules yet (P1.3 / P9 will). Public entrypoint `pub fn run() -> anyhow::Result<()>`
     — or return `Result<(), Box<dyn Error>>` if not adding `anyhow` (implementer's call; note it in
     the impl notes). It creates the winit `EventLoop`, constructs the `App` handler, and calls
     `event_loop.run_app(&mut app)`.
  2. [x] Define `struct App` holding: `structs: GameStructs`, `mem: GameMemory`, `dolphin: DolphinMemoryAccess`
     (plain owned fields — NO `static`/`OnceCell`/`lazy_static`), and an `Option<AppWindow>` for the
     render state that only exists after `resumed`. `App::new()` does the current `main()` body:
     `GameMemory::new()`, `DolphinMemoryAccess::new()`, `GameStructs::new_empty()` +
     `load_from_dir("prime_defs")`; stash the load `Result<(), String>` (or a resolved status string)
     on `App` so the egui window can display it. Keep/print the `Loaded N structs and M enums` line
     to stdout here as well (fold the headless print into app init — do not keep a separate headless
     path).
  3. [x] Define `struct AppWindow` (the wgpu + egui render state, created in `resumed`): `Arc<winit::window::Window>`,
     `wgpu::Instance`, `wgpu::Surface<'static>`, `wgpu::Adapter`, `wgpu::Device`, `wgpu::Queue`,
     `wgpu::SurfaceConfiguration`, `egui::Context`, `egui_winit::State`, `egui_wgpu::Renderer`.
     Write `AppWindow::new(event_loop: &ActiveEventLoop, ...)`:
       - create window via `event_loop.create_window(WindowAttributes::default().with_title("Prime Watch 2").with_inner_size(LogicalSize::new(1200, 800)))`, wrap in `Arc`.
       - `wgpu::Instance::new` (default backends), `instance.create_surface(window.clone())`.
       - `pollster::block_on` an async block that does `instance.request_adapter(&RequestAdapterOptions { compatible_surface: Some(&surface), .. })` then `adapter.request_device(&DeviceDescriptor::default(), None)` — match the exact wgpu 30.0.1 signatures (adapter/device request return types changed across wgpu versions; check `cargo doc`/compiler).
       - pick a surface format: `surface.get_capabilities(&adapter).formats[0]` (prefer an sRGB one if present), build `SurfaceConfiguration` sized to `window.inner_size()` with `PresentMode::Fifo`, `surface.configure(&device, &config)`.
       - `egui::Context::default()`; `egui_winit::State::new(ctx.clone(), egui::ViewportId::ROOT, &window, Some(window.scale_factor() as f32), None, Some(device.limits().max_texture_dimension_2d as usize))` — verify arg list against egui-winit 0.36.
       - `egui_wgpu::Renderer::new(&device, config.format, None /* no depth yet */, 1 /* msaa */, false /* dithering */)` — verify against egui-wgpu 0.36.
  4. [x] `impl ApplicationHandler for App`:
       - `resumed`: if `self.window.is_none()`, build `AppWindow` and store it; call `window.request_redraw()`.
       - `window_event`: first forward the event to `egui_winit::State::on_window_event(&window, &event)`
         and short-circuit on `response.consumed` where appropriate. Handle:
           - `WindowEvent::CloseRequested` → `event_loop.exit()`.
           - `WindowEvent::Resized(size)` → update `config.width/height` (clamp to >= 1), `surface.configure`, `window.request_redraw()`.
           - `WindowEvent::ScaleFactorChanged` → nothing special needed beyond egui_winit handling; request redraw.
           - `WindowEvent::RedrawRequested` → call `self.render()`.
       - `about_to_wait`: `window.request_redraw()` (continuous redraw, mirrors the C++ `while` loop). Leave a `// TODO(P9): only redraw on demand / frame-pace` comment.
  5. [x] `AppWindow::render(&mut self, app_status: &str)` (or pass the needed status in):
       - `surface.get_current_texture()`; on `SurfaceError::Outdated/Lost` reconfigure and return, on `OutOfMemory` exit.
       - build a `TextureView`, a `CommandEncoder`.
       - egui: `let raw_input = state.take_egui_input(&window);` then
         `let full_output = ctx.run(raw_input, |ctx| { egui::Window::new("Prime Watch").show(ctx, |ui| { ui.label(app_status); }); });`
         (when defs failed to load, show the error string instead — one `egui::Window` either way, matching C++ `NOT LOADED` window).
       - `state.handle_platform_output(&window, full_output.platform_output);`
       - `let tris = ctx.tessellate(full_output.shapes, full_output.pixels_per_point);`
       - `for (id, delta) in &full_output.textures_delta.set { renderer.update_texture(&device, &queue, *id, delta); }`
       - `let screen_desc = egui_wgpu::ScreenDescriptor { size_in_pixels: [config.width, config.height], pixels_per_point: full_output.pixels_per_point };`
       - `renderer.update_buffers(&device, &queue, &mut encoder, &tris, &screen_desc);`
       - begin a `RenderPass` with `LoadOp::Clear(wgpu::Color::BLACK)` (== C++ `glClearColor(0,0,0,1)`), `StoreOp::Store`, no depth attachment.
       - `renderer.render(&mut pass, &tris, &screen_desc);` (may need `forget_lifetime()` on the pass in wgpu 30 — check).
       - drop the pass; `for id in &full_output.textures_delta.free { renderer.free_texture(id); }`
       - `queue.submit([encoder.finish()]); window.pre_present_notify(); frame.present();`
  6. [x] Rewrite `src/main.rs` to just: `mod mem; mod structs; mod app;` and `fn main() { app::run().unwrap() }`
     (or propagate the error). Remove the old `read_u16` / `get_dolphin_pids` demo prints — those were
     scratch; fold the meaningful `Loaded N structs` print into `App::new` per step 2. Note in impl
     notes that the `mem.read_u16(0x80000000)` / `dma.get_dolphin_pids()` demo calls are dropped.
  7. [x] `cargo build` + `cargo clippy --all-targets` clean; `cargo fmt` no diff. Do NOT expect to run
     the window in this env (no display — see Watch for).

  **Watch for:**
  - No display in this environment: `cargo build` + `cargo clippy --all-targets` + `cargo fmt --check`
    are the only gates. `cargo run` will fail to open a window / pick a wgpu adapter here — the human
    runs it manually. State this in the impl notes; do not add headless fallbacks or a software
    adapter to make it "pass" here.
  - No-globals convention: `GameStructs` / `GameMemory` / `DolphinMemoryAccess` are owned fields on
    `App`, threaded by `&`/`&mut` into methods. Do not reintroduce Bevy-style ambient state or any
    `static mut` / `OnceCell` / `lazy_static`. A `Ctx<'a>` struct is P4.5 — don't pre-build it, but
    keep the fields grouped so it's a trivial later change.
  - BE conversion / `& 0x7FFFFFFF` masking / bitfield semantics: N/A for this task — no new memory
    reads. If tempted to display a memory value in the egui window, don't; keep it to the load-status
    string.
  - winit 0.30 `ApplicationHandler`: window + GPU resources must be created in `resumed`, not before
    the event loop runs (Wayland/macOS require it). `App` starts with `window: None`.
  - `Surface<'static>` needs the window behind an `Arc` (`instance.create_surface(window.clone())`),
    not a borrow — otherwise lifetime hell.
  - wgpu 30.0.1 vs. older tutorials: `request_adapter` / `request_device` return types, `Instance::new`
    arg shape, `RenderPass` lifetime (`forget_lifetime`), and `create_surface` signature all shifted
    between wgpu 22→30. Trust the compiler / `cargo doc -p wgpu`, not memorized snippets.
  - egui-winit / egui-wgpu 0.36 constructor arg lists (`State::new`, `Renderer::new`) changed recently
    (added `dithering`, theme option). Verify against the built docs.
  - 2-space rustfmt (`.rustfmt.toml`).
  - Scope guard: no 3D texture / paint-callback pattern (that's P1.3), no input handling beyond
    close/resize (that's P9), no per-frame memory parse (P3+). One egui window showing load status,
    over a black clear color. Nothing more.
  - `.cargo/cargo.toml` is inert (cargo only reads `.cargo/config.toml`) — if the mold linker is
    missing the build may fail; that's a pre-existing env note, not this task's problem, but mention
    it if hit.

  **Done when:**
  - `cargo build` and `cargo clippy --all-targets` finish with no errors and no new warnings.
  - `cargo fmt --check` is clean.
  - `grep -n "OnceCell\|lazy_static\|static mut" src/app.rs` is empty; `App` holds `GameStructs` /
    `GameMemory` / `DolphinMemoryAccess` as plain fields.
  - `src/main.rs` is reduced to module decls + a one-line `app::run()` call; the old scratch
    `read_u16` / `get_dolphin_pids` prints are gone; the `Loaded N structs and M enums` line is
    emitted from `App::new`.
  - Manual (human, has a display): `cargo run` opens a 1200x800 "Prime Watch 2" window with a black
    background and one egui window reading `Loaded 211 structs and 36 enums`; resizing the window does
    not panic or stretch/garble; closing it exits cleanly.
  - Committed with the `TASKS.md` promotion as `port(P1.2): bare winit/wgpu/egui window`.

  **Manual verification (human, needs a display — none in this env):**
  - [ ] `cargo run` opens a 1200x800 window titled "Prime Watch 2" with a black background.
  - [ ] One egui window titled "Prime Watch" shows `Loaded 211 structs and 36 enums`.
  - [ ] Resizing the window does not panic and the egui content is not stretched/garbled.
  - [ ] Closing the window exits the process cleanly (exit code 0).
  - [ ] Stdout still prints `Loaded 211 structs and 36 enums` on startup.

  **Implementation notes (P1.2):**
  - New `src/app.rs`; `src/main.rs` is now `mod app; mod mem; mod structs;` + `fn main() -> Result<(),
    Box<dyn std::error::Error>> { app::run() }`. Dropped the scratch `mem.read_u16(0x80000000)` /
    `dma.get_dolphin_pids()` demo prints. `run()` returns `Result<(), Box<dyn std::error::Error>>`
    (no `anyhow` dep, per the task instruction).
  - `App` owns `mem: GameMemory`, `dolphin: DolphinMemoryAccess`, `structs: GameStructs`,
    `defs_loaded: bool`, `status_text: String`, `window: Option<AppWindow>` — plain fields, no
    `static`/`OnceCell`/`lazy_static`. `App::new()` runs the old `main()` body and prints
    `Loaded N structs and M enums` (or the error) to stdout. The three game fields carry
    `#[allow(dead_code)]` with a comment naming the phase that wires each up (P2/P3/P7) — they are
    genuinely unused until then and the task requires them held now.
  - `AppWindow` holds `Arc<Window>`, `Surface<'static>`, `Device`, `Queue`, `SurfaceConfiguration`,
    `egui::Context`, `egui_winit::State`, `egui_wgpu::Renderer`. The `wgpu::Instance` and
    `wgpu::Adapter` are NOT stored (not needed after setup — deviation from the step 3 field list;
    add them back trivially if a later phase needs re-adapting). Created in `resumed`; on failure it
    logs to stderr and calls `event_loop.exit()`.
  - API deviations found against the planner's snippets (verified against installed crate sources):
    - `wgpu::Instance::new` takes an `InstanceDescriptor` (no `Default`); used `wgpu::Instance::default()`.
    - `adapter.request_device` in wgpu 30 takes a single `&DeviceDescriptor` (the old trace-path 2nd
      arg is gone); `request_adapter`/`request_device` now return `Result<_, _>` futures.
    - `surface.get_current_texture()` returns `wgpu::CurrentSurfaceTexture` (an enum), not
      `Result<SurfaceTexture, SurfaceError>`. Handled `Success`/`Suboptimal` → render,
      `Outdated`/`Lost` → reconfigure+skip, `Timeout`/`Occluded`/`Validation` → skip. No `OutOfMemory`
      variant exists.
    - Present is `queue.present(frame)` in wgpu 30, not `frame.present()`.
    - `RenderPassDescriptor` gained `multiview_mask: Option<NonZeroU32>`; `RenderPassColorAttachment`
      gained `depth_slice: Option<u32>`. Both set to `None`.
    - `egui_wgpu::Renderer::new(&device, format, RendererOptions)` — the msaa/depth/dithering args
      are now bundled in `RendererOptions` (used `::default()`, which is `dithering: true`; planner
      suggested `false` — cosmetic, revisit when 3D lands in P8).
    - `egui::Context::run` is renamed; egui 0.36's `run_ui` closure yields `&mut Ui` not `&Context`.
      Used `ctx.begin_pass(raw_input)` / `egui::Window::show(&ctx, ..)` / `ctx.end_pass()` — a closer
      match to the C++ `NewFrame`/`Render` structure anyway.
    - `epaint 0.36` `textures_delta.set` is `HashMap<TextureId, SmallVec<[ImageDelta; 1]>>` (was
      `Vec<(TextureId, ImageDelta)>`); iterate the inner smallvec.
    - egui-wgpu `Renderer::render` wants `&mut RenderPass<'static>`; used `.forget_lifetime()`.
  - Clear color is `wgpu::Color::BLACK` == C++ `glClearColor(0,0,0,1)`. Fallback window titled
    "NOT LOADED" with the four C++ text lines when defs fail; "Prime Watch" + status label when they
    load. No "Reload" button (C++ has one) — deferred; would need `&mut` defs access inside the egui
    closure. Continuous redraw via `about_to_wait` with a `// TODO(P9)` to frame-pace.
  - Gates: `cargo build` clean; `cargo clippy --all-targets` clean — 0 warnings from `app.rs`/`main.rs`
    (37 total, all pre-existing in `src/mem/**`, `src/structs/**`, `bstruct/`); `cargo fmt --check`
    exit 0; `cargo test` passes (0 tests — P1.2 has no test in "Done when", window is manual-only).
    `cargo run` not exercised here (no display / no GPU adapter — see Watch for).

  **Review (P1.2):** `cargo fmt --check` exit 0; `cargo build` clean; `cargo clippy --all-targets`
  37 warnings, all pre-existing in `src/mem/**` / `src/structs/**` / `src/bstruct_link.rs`, zero from
  `app.rs`/`main.rs`; `cargo test` 0 pass / 0 fail. No `static`/`OnceCell`/`lazy_static` in `app.rs`;
  `GameStructs`/`GameMemory`/`DolphinMemoryAccess` are plain owned `App` fields. `main.rs` is module
  decls + `app::run()`; scratch `read_u16`/`get_dolphin_pids` prints gone; `Loaded N structs and M
  enums` now printed from `App::new`. Scope clean — only `src/app.rs`, `src/main.rs`, `TASKS.md`.
  Spot-checked the reported unusual wgpu-30 / egui-wgpu-0.36 API shapes against the installed crate
  sources under `~/.cargo/registry/src/`: `Surface::get_current_texture -> CurrentSurfaceTexture`
  enum (surface_texture.rs:48, variants Success/Suboptimal/Timeout/Occluded/Outdated/Lost/Validation),
  `Queue::present(SurfaceTexture)` (queue.rs:377), `Adapter::request_device(&DeviceDescriptor)`
  single-arg (adapter.rs:58), `RenderPassColorAttachment::depth_slice` / `RenderPassDescriptor::
  multiview_mask` (render_pass.rs:625,682), `egui_wgpu::Renderer::new(device, format, RendererOptions)`
  (renderer.rs:267) with `render(&mut self, &mut RenderPass<'static>, ...)` (renderer.rs:476) — the
  render path is concrete inherent-method resolution, not a generic/Any fallback. Frame order in
  `AppWindow::render` matches C++ `doFrame` (PrimeWatch.cpp:234-278): begin -> build UI -> end ->
  handle_platform_output -> tessellate -> update_texture -> update_buffers -> render pass clear BLACK
  -> egui render -> free_texture -> submit -> present. Clear color `wgpu::Color::BLACK` ==
  `glClearColor(0,0,0,1)`. "NOT LOADED" fallback window reproduces the four C++ text lines. Window/
  resize/close behaviour is manual-verify only (no display in this env — see checklist below).
  Deviation accepted: the C++ "Reload" button in the NOT LOADED window is not ported — tracked as the
  P1.2 follow-up note below (needs `&mut` defs access inside the egui closure; deferred to P9).
  - _P1.2 follow-up:_ port the C++ NOT-LOADED-window "Reload" button (PrimeWatch.cpp:250-252,
    calls `loadDefs()`). Needs `&mut GameStructs` reachable from the egui closure; fold into the
    P9 app shell when defs reloading / the main menu land.

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
