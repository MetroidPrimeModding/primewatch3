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

- [x] **P1.3** Decide + spike the egui/wgpu 3D compositing pattern (render 3D to a texture shown via
  an `egui-wgpu` paint callback / `egui::Image`). Document the chosen pattern here. — `DONE`

  **Chosen pattern: B — render 3D to an offscreen wgpu texture, show it via `egui::Image`.**

  Each frame the 3D scene renders into an app-owned offscreen color+depth target, then the color
  texture is handed to egui as a user texture (`egui_wgpu::Renderer::register_native_texture` on
  first use, `update_egui_texture_from_wgpu_texture` to reuse the same `TextureId` every frame
  afterwards — that call rebuilds the bind group from the current view, so it also covers resize)
  and drawn as an `egui::Image` inside a panel. The egui pass still targets the swapchain and
  clears it to black exactly as today (C++ `glClearColor(0,0,0,1)`).

  Rationale:
  - The world view owns its own depth buffer, camera, MSAA choice, and clear color. An offscreen
    target keeps all of that isolated from egui's render pass instead of interleaving pipeline
    state inside it (pattern A / `CallbackTrait`).
  - Sizing/lifetime is easy to reason about: the target is resized to the panel's allocated size;
    there is no clip-rect/scissor math and no "callback runs inside egui's pass with egui's
    bind groups still bound" surprises.
  - It matches how `WorldRenderer` is already shaped in C++ (`render()` builds its own projection
    from `fov/aspect/zNear/zFar` and does a full clear) — Phase 8 ports `WorldRenderer::render`
    into `SpikeScene::render`'s place with minimal API disruption.
  - Tradeoffs accepted: one extra full-screen texture + blit's worth of bandwidth per frame, one
    frame of latency is possible if the panel resize and the re-register race (mitigated by
    resizing the target *before* the scene pass, from last frame's panel size), and the color
    target must be `wgpu::TextureFormat::Rgba8Unorm` (egui-wgpu hard-requires it for
    `register_native_texture`) rather than the surface's sRGB format — the spike must pick
    `Rgba8Unorm` and note the gamma implication for Phase 8.
  - Pattern A stays available if a perf problem shows up later; nothing above `SpikeScene` needs
    to know which is used as long as the P8 renderer keeps the "give me an encoder + a target
    size, I hand you back a `TextureView`" contract.

  Note for P8/P9: whether the world view ends up as a full-window `CentralPanel` background with
  floating inspector windows (like C++ `PrimeWatch::doFrame`) or a docked side panel is a *layout*
  decision — the offscreen-texture mechanism is identical either way. The spike uses a simple
  dedicated `egui::Window`/`SidePanel` to prove the plumbing.

  **Port from:**
  - `../primewatch2/src/PrimeWatch.cpp:PrimeWatch::doFrame` (lines 235-278) — frame order: memory
    parse → egui new-frame → build UI → clear `(0,0,0,1)` + `GL_DEPTH_BUFFER_BIT` + `GL_MULTISAMPLE`
    → `worldRenderer.render(...)` → egui render. The spike keeps this order with the 3D pass
    redirected to the offscreen target.
  - `../primewatch2/src/world/WorldRenderer.cpp:WorldRenderer::render` (lines 248-407) — reference
    only, NOT ported here: shows the world pass owns its projection (`glm::perspective(fov, aspect,
    zNear, zFar)`, line 259/291), its view matrix, and does its own clear. The spike stubs this
    with a single rotating primitive; the real port is P8.4.
  - `../primewatch2/src/gl/OpenGLShader.cpp` + the inline `meshVertShader` in `WorldRenderer.cpp`
    (`uniform mat4 projection; gl_Position = projection * view * model * vec4(aPos,1.0)`) — target
    shape for the spike's trivial WGSL shader (one MVP uniform).
  - `../primewatch2/src/PrimeWatch.cpp:framebuffer_size_cb` / `updateWindowSize` (~lines 485-492) —
    resize path; here it drives the offscreen target size via the panel rect, not the surface.
  - Current Rust: `src/app.rs` — `AppWindow` (fields, `new`, `resize`, `render`). The spike adds a
    `scene` field and a `scene.render()` call before the egui pass in `AppWindow::render`.

  **Steps:**
  1. [x] New module `src/scene.rs` (`mod scene;` in `src/main.rs`). Define
     `pub struct SpikeScene` holding: color texture + view (`Rgba8Unorm`, usage
     `RENDER_ATTACHMENT | TEXTURE_BINDING`), depth texture + view (`Depth32Float`,
     `RENDER_ATTACHMENT`), `wgpu::RenderPipeline`, an MVP uniform `wgpu::Buffer` + `BindGroup`,
     `size: (u32, u32)`, and a rotation accumulator (`start: std::time::Instant` or `angle: f32`).
     No `static` / `OnceCell` / `lazy_static`; the scene is owned by `AppWindow`.
  2. [x] `SpikeScene::new(device: &wgpu::Device, size: (u32, u32)) -> Self`: clamp size to `>=1`,
     create the two textures, an inline WGSL shader (vertex pulls 3 hard-coded positions/colors or a
     small cube vertex buffer; fragment passes colour through), a pipeline with the depth-stencil
     state (`Depth32Float`, `depth_write_enabled: true`, `Less`), a 64-byte uniform buffer for one
     `mat4`, and the bind group. MSAA = 1 for the spike (leave a `// TODO(P8): MSAA` note).
  3. [x] `SpikeScene::resize(&mut self, device: &wgpu::Device, size: (u32, u32)) -> bool`: if
     `size` (clamped `>=1`) differs from `self.size`, recreate color+depth textures/views, store the
     new size, return `true` (caller must re-register the egui texture). Otherwise return `false`.
  4. [x] `SpikeScene::render(&mut self, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder)`:
     advance the angle from elapsed time, build `proj * view * model` with `glam`
     (`Mat4::perspective_rh(60°, w/h, 0.1, 100.0)`, a fixed eye via `Mat4::look_at_rh`,
     `Mat4::from_rotation_y(angle)`), `queue.write_buffer` the uniform (`.to_cols_array()`), then a
     render pass on the offscreen color view with `LoadOp::Clear` to a *distinct* colour (e.g.
     `Color { r: 0.05, g: 0.05, b: 0.12, a: 1.0 }` — visibly not the black surface) + depth clear
     `1.0`, bind pipeline + group, draw.
  5. [x] `src/app.rs` `AppWindow`: add `scene: SpikeScene` and
     `scene_texture: Option<egui::TextureId>`. In `AppWindow::new`, construct `SpikeScene::new` with
     an initial size (e.g. `(800, 600)`); leave `scene_texture: None`.
  6. [x] `src/app.rs` `AppWindow::render`: after building `encoder` and *before* the egui
     `begin_pass`, call `self.scene.render(&self.queue, &mut encoder)`. Then register or update the
     egui texture: if `scene_texture` is `None` or `scene.resize(..)` returned `true` last step, call
     `register_native_texture(&device, &color_view, FilterMode::Linear)` and store the id; else
     `update_egui_texture_from_wgpu_texture(&device, &color_view, FilterMode::Linear, id)`.
  7. [x] `src/app.rs` `AppWindow::render` UI: inside `begin_pass`/`end_pass`, in addition to the
     existing status window, add an `egui::Window::new("World")` (or `SidePanel`) that does
     `let avail = ui.available_size(); ui.image(egui::load::SizedTexture::new(id, avail));` and
     records `avail` (rounded, `* pixels_per_point`) as the desired scene size. After `end_pass`,
     call `self.scene.resize(&self.device, desired_size)` so next frame's pass matches the panel —
     recreate + re-register happens on the following frame (documented one-frame lag).
  8. [x] Keep the egui swapchain pass exactly as now: `LoadOp::Clear(wgpu::Color::BLACK)`,
     `renderer.render(&mut pass.forget_lifetime(), ..)`. The scene pass is a separate pass in the
     same encoder, submitted in the same `queue.submit`.
  9. [x] `cargo fmt` (2-space), `cargo build`, `cargo clippy --all-targets` — all clean, no new
     warnings. Add the manual-verification checklist entries (below) — the window cannot be
     exercised in this environment (no display / no GPU adapter).
  10. [x] Fill in the **Implementation notes** with any wgpu-30 / egui-wgpu-0.36 API deviations
     found (mirroring the P1.2 notes style), and confirm the "Chosen pattern" section above still
     matches what was built.

  **Watch for:**
  - BE conversion location / `& 0x7FFFFFFF` masking / explicit `Ctx` / bitfield semantics: all N/A
    — the spike does zero memory reads. Do not display any `mem`/`dolphin` value; keep it to a
    rotating primitive. `Ctx<'a>` is still P4.5 — don't pre-build it.
  - No globals: `SpikeScene` is a plain field on `AppWindow`; the rotation clock is a field, not a
    `static`. `grep -n "OnceCell\|lazy_static\|static mut" src/scene.rs src/app.rs` must stay empty.
  - egui-wgpu hard requirement: the offscreen color texture **must** be
    `wgpu::TextureFormat::Rgba8Unorm` (see `egui-wgpu-0.36.1/src/renderer.rs:770,818`). Not the
    surface's sRGB format, not `Bgra8*`. Note the gamma implication for P8 in the impl notes.
  - Offscreen color texture usage must include `TEXTURE_BINDING` as well as `RENDER_ATTACHMENT`, or
    `register_native_texture` / sampling fails.
  - Don't `register_native_texture` every frame (leaks a sampler + bind group each call): register
    once, then `update_egui_texture_from_wgpu_texture` with the stored `TextureId`, re-registering
    only when the target is recreated on resize.
  - wgpu 30 pass descriptors: `RenderPassColorAttachment.depth_slice: None`,
    `RenderPassDescriptor.multiview_mask: None` (already handled in `app.rs` — match it for the
    scene pass). `egui_wgpu::Renderer::render` wants `&mut RenderPass<'static>` → `.forget_lifetime()`.
  - Panel size can be zero (collapsed window / dragged tiny) — clamp to `>=1` before creating
    textures or configuring the pass; `Mat4::perspective_rh` with aspect `0` produces NaNs.
  - Scope guard: this is a *spike*. No `CollisionMesh`, no `ShapeGenerator`, no camera modes, no
    `WorldRenderer`, no memory-driven geometry, no MSAA, no lighting. One rotating triangle or cube.
    Phase 8 does the real thing.
  - Possible vertical flip: egui `Image` UV origin is top-left and so is the wgpu render target —
    should line up, but eyeball it in manual verification and flip UVs in the `Image` if the
    primitive renders upside down.
  - 2-space rustfmt (`.rustfmt.toml`); `edition = "2024"`.
  - No carried-over C++ bug in scope here.

  **Done when:**
  - `cargo build` and `cargo clippy --all-targets` are clean with no new warnings; `cargo fmt --check`
    is clean.
  - `grep -n "OnceCell\|lazy_static\|static mut" src/scene.rs src/app.rs` prints nothing.
  - `src/scene.rs` exists with `SpikeScene::{new,resize,render}`; `AppWindow` renders the scene to an
    offscreen `Rgba8Unorm` target and shows it via `egui::Image` in a panel, with the swapchain
    still cleared to black by the egui pass.
  - The **Chosen pattern** section above is committed as the documented decision (this is the
    primary deliverable of P1.3).
  - Committed with the `TASKS.md` promotion as `port(P1.3): spike offscreen-texture 3D compositing`.

  **Manual verification (human, needs a display — none in this env):**
  - [ ] `cargo run` opens the window; a "World" panel/window contains a rotating triangle/cube on a
    dark-blue clear, distinct from the black window background.
  - [ ] Resizing the OS window (and the panel) resizes the 3D content to fit within ~1 frame,
    without stretching, garbling, aspect distortion, or panic.
  - [ ] Collapsing / shrinking the "World" panel to near-zero does not panic.
  - [ ] The existing "Prime Watch" status window still shows `Loaded 211 structs and 36 enums`.
  - [ ] Closing the window exits cleanly (exit code 0).

  **Implementation notes (P1.3):**
  - New `src/scene.rs` (`mod scene;` added to `src/main.rs`). `pub struct SpikeScene` with
    `new` / `resize` / `render` / `color_view`, owned as a plain field on `AppWindow` — no
    `static` / `OnceCell` / `lazy_static` (`grep` clean). Rotation is driven by a `start:
    std::time::Instant` field.
  - Offscreen target: `Rgba8Unorm` colour (`RENDER_ATTACHMENT | TEXTURE_BINDING`) + `Depth32Float`
    depth (`RENDER_ATTACHMENT`), MSAA = 1 (`// TODO(P8): MSAA`). Scene pass clears colour to
    `Color { r: 0.05, g: 0.05, b: 0.12, a: 1.0 }` and depth to `1.0`; depth test `Less`,
    depth write on. Draws an indexed 8-vertex / 36-index cube with per-vertex colour — the
    back faces are behind the front faces so the depth buffer is genuinely exercised
    (`cull_mode: Back` as well).
  - **Gamma note for P8:** the egui composite target is *linear* `Rgba8Unorm` (egui-wgpu
    hard-requires it — `renderer.rs:770`), not the surface's sRGB format. Colours written by the
    scene shader are not gamma-encoded, so the real `WorldRenderer` port must do its own
    linear→sRGB handling (or accept the slightly-dark look) when it lands here.
  - `AppWindow`: added `scene: SpikeScene` (initial size `(800, 600)`) and
    `scene_texture: Option<egui::TextureId>`. `render()` order now: get frame → encoder →
    `scene.render(&queue, &mut encoder)` → register-or-update the egui user texture →
    `begin_pass` → "Prime Watch"/"NOT LOADED" status window + new `egui::Window::new("World")`
    with `ui.image(SizedTexture::new(id, ui.available_size()))` → `end_pass` → egui swapchain
    pass unchanged (`LoadOp::Clear(BLACK)`, `.forget_lifetime()`) → submit (scene + egui in one
    `queue.submit`) → present → `scene.resize(panel_size * pixels_per_point)` for next frame
    (documented one-frame lag).
  - Deviation from step 6: do **not** re-`register_native_texture` on resize. `SpikeScene::resize`
    still returns `bool` (target recreated), but `AppWindow` ignores it and always calls
    `update_egui_texture_from_wgpu_texture` after the first registration — that call rebuilds the
    egui bind group from the current `TextureView` every frame, so it transparently picks up a
    resized target with zero leaked bind groups (the "register once" guidance in Watch-for,
    taken to its logical conclusion).
  - wgpu 30.0.1 API deviations found (this crate's `wgpu 30.0.1` is a newer API snapshot than
    older tutorials / the P1.2 notes — verified against the installed crate source):
    - `PipelineLayoutDescriptor`: `bind_group_layouts: &[Option<&BindGroupLayout>]` (was
      `&[&BindGroupLayout]`); `push_constant_ranges` is gone, replaced by `immediate_size: u32`
      (set to `0`).
    - `DepthStencilState`: `depth_write_enabled: Option<bool>` and
      `depth_compare: Option<CompareFunction>` (were bare `bool` / `CompareFunction`).
    - `VertexState.buffers: &[Option<VertexBufferLayout>]` (wrap the layout in `Some`).
    - `VertexState` / `FragmentState`: `entry_point: Option<&str>` + `compilation_options`
      (`PipelineCompilationOptions::default()`).
    - `RenderPipelineDescriptor`: has `multiview_mask: Option<NonZeroU32>` and
      `cache: Option<&PipelineCache>` (both `None`); no `multiview` field.
    - Render-pass descriptors match the P1.2 shapes: `RenderPassColorAttachment.depth_slice: None`,
      `RenderPassDescriptor.multiview_mask: None`.
  - glam 0.33.6 deviation: `Mat4::perspective_rh` / `Mat4::look_at_rh` are **deprecated** in this
    version (would trip the "no new warnings" gate). Used
    `glam::camera::rh::proj::directx::perspective` (RH world, [0,1] clip depth — wgpu convention)
    and `glam::camera::rh::view::look_at_mat4` instead.
  - `egui-wgpu 0.36.1` `register_native_texture(&mut self, &Device, &TextureView, FilterMode)
    -> TextureId` and `update_egui_texture_from_wgpu_texture(&Device, &TextureView, FilterMode,
    TextureId)` used as the planner described — signatures confirmed against
    `egui-wgpu-0.36.1/src/renderer.rs:771,792`.
  - No `bytemuck` dependency added: a tiny local `as_bytes<T: Copy>(&[T]) -> &[u8]` helper (one
    documented `unsafe` block, `u8` align-1) casts the static vertex/index arrays and the MVP
    `[f32; 16]` for upload.
  - Gates: `cargo fmt --check` exit 0; `cargo build` clean; `cargo clippy --all-targets` reports
    zero warnings in `src/scene.rs` / `src/app.rs` (all remaining warnings are pre-existing in
    `src/mem/**`, `src/structs/**`, `src/bstruct_link.rs`, `bstruct/`); `cargo test` 0 pass /
    0 fail (no offline test possible — needs a GPU + display, covered by the manual checklist).
    Window not exercised in this env (no display / no adapter).

  **Review (P1.3):** `cargo fmt --check` exit 0; `cargo build` clean; `cargo clippy --all-targets`
  and `cargo test` (0/0) clean — every warning is pre-existing in `src/mem/**`, `src/structs/**`,
  `src/bstruct_link.rs`, `bstruct/`; zero from `src/scene.rs` / `src/app.rs`. No
  `static`/`OnceCell`/`lazy_static` in either file (consts only). Scope clean — only `src/scene.rs`,
  `src/app.rs`, `src/main.rs`, `TASKS.md`. Chosen-pattern-B decision is committed as the primary
  deliverable. Frame order in `AppWindow::render` matches C++ `PrimeWatch::doFrame` (PrimeWatch.cpp
  235-278) adapted for the offscreen pattern: scene pass encoded first -> egui texture
  register/update -> begin_pass -> status window + "World" `ui.image` -> end_pass -> swapchain pass
  `LoadOp::Clear(BLACK)` (== `glClearColor(0,0,0,1)`) -> egui render -> single `queue.submit`
  (scene + egui) -> present -> `scene.resize` for next frame (documented one-frame lag). Depth
  buffer genuinely wired: `Depth32Float` attachment, `depth_compare: Some(Less)`,
  `depth_write_enabled: Some(true)`, depth cleared to `1.0`, indexed 36-index cube. Offscreen colour
  is `Rgba8Unorm | RENDER_ATTACHMENT | TEXTURE_BINDING` per egui-wgpu's hard requirement; gamma note
  for P8 recorded.

  Verified the reported unusual API paths against installed crate sources under
  `~/.cargo/registry/src/`:
  - `glam::camera::rh::proj::directx::perspective` (glam-0.33.6 `src/camera/rh/proj.rs`) — RH Y-up
    view-space input, NDC Z in [0,1], Y-up: exactly the wgpu convention. Its `camera_impl` const
    generics are `<true, true, false>` (rh, zero-to-one, y-up). This is the *exact* replacement the
    deprecation note on `Mat4::perspective_rh` names ("use the `glam::camera::rh::proj::directx::
    perspective` function instead", `src/f32/scalar/mat4.rs:1031`). Not dubious — glam-prescribed.
  - `glam::camera::rh::view::look_at_mat4` (glam-0.33.6 `src/camera/rh/view.rs:27`) — full RH view
    transform from eye/center/up; the deprecation note on `Mat4::look_at_rh` names it verbatim
    (`mat4.rs:847`).
  - wgpu-30 `PipelineLayoutDescriptor` (wgpu-30.0.1 `src/api/pipeline_layout.rs:33`): fields are
    `bind_group_layouts: &[Option<&BindGroupLayout>]` + `immediate_size: u32`; `push_constant_ranges`
    is gone. `DepthStencilState` (wgpu-types-30.0.1 `src/render.rs:831,837`):
    `depth_write_enabled: Option<bool>`, `depth_compare: Option<CompareFunction>`.
  - `egui_wgpu::Renderer::{register_native_texture, update_egui_texture_from_wgpu_texture}`
    (egui-wgpu-0.36.1 `src/renderer.rs:771,792`) — signatures match the call sites.

  Manual verification still required by the human (no display / GPU in this env) — the checklist
  above applies. Additionally eyeball that the cube is not rendered inside-out: winding vs
  `cull_mode: Back` was not verified analytically; worst case one face-set is culled and the far
  faces show instead — still a visible rotating cube, but note it.

## Phase 2 — Memory access (ports `src/MemoryAccess.cpp`)

- [x] **P2.1** Real Linux/macOS `shm_open` + `mmap` bodies in `src/mem/dolphin_memory.rs`
  (`libc`/`nix`). Delete `src/mem/memory_access.rs` (dead duplicate). — `DONE`

  **Port from:**
  - `../primewatch2/src/MemoryAccess.cpp:73` — `attachToProcess(int pid)` (Linux): `shm_open("/dolphin-emu.<pid>", O_RDWR, 0600)` → `mmap(nullptr, 0x2040000, PROT_READ|PROT_WRITE, MAP_SHARED, fd, 0)` → `close(fd)`.
  - `../primewatch2/src/MemoryAccess.cpp:102` — `detachFromProcess()` (Linux): `munmap(emuRAMAddressStart, 0x2040000)`.
  - `../primewatch2/src/MemoryAccess.cpp:115` — `getAttachedPid()`.
  - `../primewatch2/src/MemoryAccess.cpp:119` — `getRealPtr(uint32_t)`: `masked = address & 0x7FFFFFFF; if masked > DOLPHIN_MEMORY_SIZE return 0; return masked`.
  - `../primewatch2/src/MemoryAccess.cpp:127` — `dolphin_memcpy(void* dest, size_t offset, size_t size)`: clamp `size` to `DOLPHIN_MEMORY_SIZE`, `memcpy(dest, emuRAMAddressStart + getRealPtr(offset), size)`.
  - `../primewatch2/src/MemoryAccess.cpp:392`–`432` — `__APPLE__` `attachToProcess` / `detachFromProcess`: byte-identical to the Linux branch (share one impl).
  - `../primewatch2/src/MemoryAccess.hpp:7` — `constexpr int DOLPHIN_MEMORY_SIZE = 0x1800000` (snapshot/copy cap).
  - shm mapping span `0x2040000`: `MemoryAccess.cpp:74`, `:103` (`constexpr size_t size`).
  - `../primewatch2/src/GameMemory.cpp:17` — `updateFromDolphin()`: the only live caller shape — `dolphin_memcpy(memory.data(), 0, 0x1800000)` (offset always 0). Wiring is P3.2, not this task.
  - `../primewatch2/src/MemoryAccess.cpp:39` — `getDolphinPids` (Linux): already ported via `sysinfo` in `get_dolphin_pids`; no change.
  - NOT ported: `beToHost16/32/64` + `hostToBe*` (`MemoryAccess.cpp:137`–`159`) — BE↔host conversion lives once in `GameMemory` (`from_be_bytes`); this layer only moves raw bytes.

  **Steps:**
  1. [x] `Cargo.toml`: add `libc = "0.2"` to `[dependencies]` (already transitively at 0.2.189; `nix` is not
     in the tree, `memmap2` still needs `libc` for `shm_open` — so use `libc` directly for
     `shm_open`/`mmap`/`munmap`/`close`).
  2. [x] `git rm src/mem/memory_access.rs` — orphaned earlier draft: not declared in `src/mem/mod.rs`, an
     exact duplicate of `dolphin_memory.rs` with the methods made private. Confirm `grep -rn
     "memory_access" src/` is empty afterwards and `src/mem/mod.rs` is untouched (it never referenced it).
  3. [x] `dolphin_memory.rs`: add `const DOLPHIN_SHM_SIZE: usize = 0x2040000;` (the `mmap`/`munmap` span,
     C++ local `size`). Keep `DOLPHIN_MEMORY_SIZE = 0x1800000` as the copy cap. Document both.
  4. [x] Collapse the OS `#[cfg]` field/ctor duplication: gate `emu_ram_address_start: *mut u8` on
     `#[cfg(any(target_os = "linux", target_os = "macos"))]` (Linux + macOS share the POSIX path).
     Leave the Windows fields (`dolphin_proc_handle`, `emu_ram_address_start: u64`) exactly as-is for P2.2.
  5. [x] Implement `attach_to_process` under `#[cfg(any(target_os = "linux", target_os = "macos"))]`
     (port `MemoryAccess.cpp:73`): `detach_from_process()`; `CString::new(format!("/dolphin-emu.{pid}"))`;
     `fd = libc::shm_open(name.as_ptr(), libc::O_RDWR, 0o600)`, `fd < 0` → `eprintln!` with
     `std::io::Error::last_os_error()` + `return false`; `ptr = libc::mmap(null_mut(), DOLPHIN_SHM_SIZE,
     PROT_READ|PROT_WRITE, MAP_SHARED, fd, 0)`; `libc::close(fd)` on **both** success and mmap-failure
     paths (C++ does); `ptr == libc::MAP_FAILED` (compare `==`, it is `-1 as *mut c_void`, not null) →
     `eprintln!` + `return false`; store `ptr as *mut u8`, set `attached_pid = pid`, `return true`.
     Every `unsafe` block gets a `// SAFETY:` note.
  6. [x] Implement `detach_from_process` under the same cfg (port `MemoryAccess.cpp:102`): if
     `!emu_ram_address_start.is_null()` → `libc::munmap(ptr as *mut c_void, DOLPHIN_SHM_SIZE)`, on `< 0`
     `eprintln!` and continue (do **not** `std::process::exit(4)` like C++ line 108), then null the
     pointer and `attached_pid = -1`.
  7. [x] Implement `dolphin_memcpy` under the same cfg (port `MemoryAccess.cpp:127` + `getRealPtr`):
     `emu_ram_address_start.is_null()` → `return false`; `real = offset & 0x7FFF_FFFF`;
     `real > DOLPHIN_MEMORY_SIZE` → `return false` (deviation: C++ `getRealPtr` silently returns 0 —
     latent bug; the only caller passes offset 0, so refusing OOB is safe and clearer — note it in impl
     notes); `n = size.min(DOLPHIN_MEMORY_SIZE).min(dest.len()).min(DOLPHIN_SHM_SIZE - real)`;
     `unsafe { std::ptr::copy_nonoverlapping(self.emu_ram_address_start.add(real), dest.as_mut_ptr(), n) }`;
     `return true`. This replaces the current linux body (`dolphin_memory.rs:132`) which copies
     `size.min(DOLPHIN_MEMORY_SIZE)` bytes into `dest` **without** bounding by `dest.len()` — a buffer
     overrun for a short `dest`.
  8. [x] Add `impl Drop for DolphinMemoryAccess { fn drop(&mut self) { self.detach_from_process(); } }` so
     the mapping is always unmapped (C++ relies on an explicit `detachFromProcess` call). `detach_from_process`
     must be a safe no-op when nothing is attached on every target — leave the Windows arm's
     `todo!("CloseHandle")` replaced with a `// P2.2:` comment + the existing null-guarded field reset so
     `Drop` on Windows cannot panic; do **not** implement `CloseHandle` here.
  9. [x] Keep public signatures stable for `src/app.rs`: `get_dolphin_pids(&mut self) -> Vec<sysinfo::Pid>`,
     `attach_to_process(&mut self, pid: i32) -> bool`, `dolphin_memcpy(&self, &mut [u8], usize, usize) -> bool`,
     `get_attached_pid(&self) -> i32`. Callers convert `Pid` via `pid.as_u32() as i32`.
  10. [x] `cargo fmt` (2-space), `cargo build`, `cargo clippy --all-targets` — clean, no new warnings.
      Windows can't be built here: eyeball that every `#[cfg(target_os = "windows")]` arm still has all
      fields initialised and no changed arity. `grep -n "todo!" src/mem/dolphin_memory.rs` should show
      only Windows-arm hits.

  **Watch for:**
  - **BE conversion stays in `GameMemory`.** Do not port `beToHost*` / `hostToBe*`; `dolphin_memcpy`
    delivers raw big-endian bytes and `game_memory.rs` does `from_be_bytes` once. No byte-swapping in
    this file.
  - **`& 0x7FFFFFFF` masking:** `getRealPtr` masks the offset before pointer arithmetic — keep that mask
    here even though `GameMemory::address_to_offset` also masks on the read path; this raw layer is
    reached independently.
  - **No globals.** C++ `MemoryAccess` uses namespace-scope statics (`attachedPid`, `emuRAMAddressStart`);
    Rust keeps them as `DolphinMemoryAccess` fields (already does). No `static` / `OnceCell` / `lazy_static`.
  - Explicit `Ctx` / bitfield semantics: N/A at this layer.
  - **Highest-risk `unsafe` in the whole port** (plan "Open risks"): every `unsafe` block needs a
    `// SAFETY:` comment; bound every copy by *both* `dest.len()` and `DOLPHIN_SHM_SIZE - real`.
  - `libc::MAP_FAILED` is `(-1isize) as *mut c_void`, not null — test with `==`.
  - `close(fd)` after `mmap` on success *and* on the mmap-failure path (C++ `MemoryAccess.cpp:92`, `:98`).
  - Do not `std::process::exit` on `munmap` failure (C++ `exit(4)`, line 108) — log and continue.
  - `DolphinMemoryAccess` holds a raw pointer → not `Send`/`Sync`; that's fine, it lives on `App` and is
    never threaded. Don't add `unsafe impl Send`.
  - Carried-over bug to fix: the current linux `dolphin_memcpy` (`dolphin_memory.rs:132`) does an
    unbounded `copy_nonoverlapping` into `dest` (ignores `dest.len()`) and clamps the source offset with
    `.min(DOLPHIN_MEMORY_SIZE)` instead of range-checking — replace per step 7.
  - 2-space rustfmt; `edition = "2024"`.
  - Scope guard: no Windows bodies (P2.2), no `GameMemory` wiring / per-frame refresh (P3.2), no `.raw`
    path changes (already in `game_memory.rs`). Just the POSIX attach/detach/copy + delete the dupe.

  **Decisions for the human before implementing:**
  - **A. Crate:** recommend `libc` (already in the dependency tree; `nix` is not; `memmap2` would still
    need `libc` for `shm_open`). Confirm or pick `nix`.
  - **B. OOB offset in `dolphin_memcpy`:** recommend `return false` (C++'s silent read-from-0 is a latent
    bug and the sole caller uses offset 0). Confirm or keep C++ behaviour.
  - **C. `Drop` impl:** recommend adding it (C++ has none but leaks the mapping without an explicit
    detach). Confirm.
  - **D. Share one impl for Linux + macOS** via `#[cfg(any(target_os = "linux", target_os = "macos"))]`
    (C++ `__APPLE__` branch is identical). Confirm, or keep them separate.

  **Done when:**
  - `cargo build` clean; `cargo clippy --all-targets` clean with no new warnings; `cargo fmt --check` clean.
  - `src/mem/memory_access.rs` is gone; `grep -rn "memory_access" src/` is empty; `src/mem/mod.rs` unchanged.
  - The Linux/macOS `attach_to_process` / `detach_from_process` / `dolphin_memcpy` bodies contain real
    `libc::{shm_open, mmap, munmap, close}` + `copy_nonoverlapping` calls; `grep -n "todo!"
    src/mem/dolphin_memory.rs` shows only Windows-arm occurrences.
  - Committed with the `TASKS.md` promotion as `port(P2.1): real POSIX shm_open/mmap Dolphin attach`.

  **Implementation notes (P2.1):**
  - `src/mem/memory_access.rs` deleted via `git rm` — was an orphaned exact-duplicate draft, never
    in `mod.rs`; `grep -rn memory_access src/` and `grep -rn MemoryAccessImpl src/` are now empty.
    `src/mem/mod.rs` untouched. Also dropped the now-orphaned `pub use DolphinMemoryAccess as
    MemoryAccessImpl;` re-export from `dolphin_memory.rs` (nothing referenced it).
  - `Cargo.toml`: added `libc = "0.2"` (resolved to the 0.2.x already in the lockfile).
  - One POSIX impl gated `#[cfg(any(target_os = "linux", target_os = "macos"))]` (C++ `__APPLE__`
    branch is byte-identical). `emu_ram_address_start: *mut u8` collapsed onto that cfg; Windows
    fields untouched. Added `#[cfg(not(any(linux, macos, windows)))]` fallback arms to
    `attach_to_process` / `dolphin_memcpy` returning `false` so the crate compiles on any target
    (previously those arms fell through and would not have compiled).
  - `attach_to_process`: real `libc::shm_open` (mode arg split by cfg — plain `0o600` on Linux
    where the 3rd param is `mode_t`, `0o600 as libc::c_int` on macOS where `shm_open` is variadic),
    `libc::mmap(NULL, DOLPHIN_SHM_SIZE, RW, MAP_SHARED, fd, 0)`, `libc::close(fd)` on both the
    success and `MAP_FAILED` paths, `== libc::MAP_FAILED` check. `attached_pid` is set only on
    success (matches C++ ordering). Every `unsafe` block has a `// SAFETY:` comment.
  - `detach_from_process`: `libc::munmap(ptr, DOLPHIN_SHM_SIZE)`; on non-zero return it logs
    `std::io::Error::last_os_error()` and continues — deliberately NOT the C++ `exit(4)`.
  - `dolphin_memcpy`: masks `offset & 0x7FFF_FFFF`; `real_offset > DOLPHIN_MEMORY_SIZE` →
    `return false` (deviation from C++ `getRealPtr`, which silently substitutes 0 — latent bug, and
    the sole live caller passes offset 0). Copy length `n = size.min(DOLPHIN_MEMORY_SIZE)
    .min(dest.len()).min(DOLPHIN_SHM_SIZE - real_offset)` — fixes the prior unbounded
    `copy_nonoverlapping` that ignored `dest.len()` (overrun for a short `dest`).
  - `const DOLPHIN_SHM_SIZE: usize = 0x2040000` added alongside the `DOLPHIN_MEMORY_SIZE = 0x1800000`
    copy cap, both doc-commented.
  - Added `impl Drop for DolphinMemoryAccess` → `detach_from_process()`, and an
    `impl Default` (delegates to `new`). Windows `detach_from_process` arm: `todo!("CloseHandle")`
    replaced with a `// P2.2:` comment + the existing null-guarded field reset, so `Drop` cannot
    panic on Windows. `grep -n "todo!" src/mem/dolphin_memory.rs` is now empty (no todo! on any arm).
  - Windows `attach_to_process` / `dolphin_memcpy` arms: `todo!(...)` replaced with `// P2.2:`
    comments + `false` returns. Windows field init/arity unchanged (can't build Windows here —
    eyeballed).
  - Public signatures unchanged; `src/app.rs` compiles untouched. `game_memory.rs` not touched
    (wiring is P3.2).
  - Gates: `cargo fmt --check` exit 0; `cargo build` clean; `cargo clippy --all-targets` — the only
    `dolphin_memory.rs` warnings (`DOLPHIN_MEMORY_SIZE` unused, `system` field unread, methods
    unused) are all pre-existing (verified by stashing the change); `detach_from_process` actually
    dropped off the dead-code list because `Drop` now calls it. `cargo test` 0 pass / 0 fail
    (no offline test possible — needs a live Dolphin; covered by the P2.3 checklist below).
  - Reviewer: check the macOS variadic `shm_open` mode cast and the `MAP_FAILED` comparison — not
    exercisable in this Linux env.

  **Review (P2.1):** Ports `MemoryAccess.cpp:73/102/119/127` (+ byte-identical `__APPLE__`
  `:392`/`:421`) — verified side by side. `attach_to_process` does `shm_open("/dolphin-emu.<pid>",
  O_RDWR, 0600)` → `mmap(NULL, 0x2040000, PROT_READ|PROT_WRITE, MAP_SHARED, fd, 0)` → `close(fd)` on
  both the success and `MAP_FAILED` paths; `== libc::MAP_FAILED` compare; fields set only on success.
  macOS mode cast `0o600 as libc::c_int` is correct (libc 0.2.189 declares Apple `shm_open` variadic;
  Linux takes `mode_t`). `dolphin_memcpy` masks `& 0x7FFF_FFFF`, rejects `real_offset >
  DOLPHIN_MEMORY_SIZE` (matches C++ `getRealPtr` `>`), and bounds the copy by
  `size`/`DOLPHIN_MEMORY_SIZE`/`dest.len()`/`DOLPHIN_SHM_SIZE - real_offset` — no underflow
  (SHM > MEMORY). Sanctioned deviations: `false` instead of C++ silent read-from-0, log instead of
  `exit(4)`, added `impl Drop`/`impl Default`, one cfg for linux+macos. No BE swapping at this layer;
  no new globals. Every `unsafe` block has a `// SAFETY:` note. `src/mem/memory_access.rs` gone,
  `grep -rn memory_access src/` empty, `src/mem/mod.rs` untouched, no `todo!` remaining, `src/app.rs`
  signatures stable. Gates: `cargo fmt --check` exit 0; `cargo build` clean; `cargo clippy
  --all-targets` 28→23 warnings (all remaining pre-existing, none in the new code — confirmed via
  `git stash`); `cargo test` 0/0. Live-Dolphin attach/copy still needs the P2.3 manual check.

  **Manual verification (P2.3 — human, needs a live Dolphin; none in this env):**
  - [ ] With MP1 running in Dolphin: `get_dolphin_pids()` returns its pid; `attach_to_process(pid)` returns `true`.
  - [ ] `dolphin_memcpy(&mut buf, 0, 0x1800000)` fills a `0x1800000`-byte buffer; `&buf[0..6] == b"GM8E01"`
    (matches `../primewatch2/mem1.raw` first bytes) and a live field (e.g. `g_stateManager` chain) reads sanely.
  - [ ] Dropping / re-attaching does not leak (check `/proc/<our-pid>/maps` shrinks after `detach_from_process`).
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
