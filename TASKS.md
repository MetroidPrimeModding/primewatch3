# Conversion tasks — remaining work

The Rust port of PrimeWatch is **code-complete through Phase 10**. Every phase (P0–P10) has
shipped and been reviewed. What remains is **manual verification that cannot be done in this
environment** — it needs a display, a live Dolphin + Metroid Prime 1, and/or a push to GitHub
Actions — plus one open software decision.

The old per-task archives (`completed_tasks/`) have been removed; the shipped-state summaries
live in the git history (`port(PX.Y): …` commits on the `rust-conversion` branch).

Status legend: `BLOCKED (reason)` · `TODO`

---

## Open software decision

### D1 — Camera FOV: degrees vs radians
`WorldRenderer::perspective()` (`src/world/renderer.rs`) passes `self.fov` (default `45`) to
`glam …::perspective` **unconverted** — a verbatim port of the C++ `glm::perspective(fov, …)`,
whose first argument is radians. `45` radians is almost certainly a latent bug carried over
from the C++. Decide whether to `to_radians()` it (and, if so, whether the C++ was also wrong
or whether some other path compensated). Easiest to judge once D2's world view is on screen.

---

## Manual verification — needs a display only (`.raw` dump is enough)

### D2 — Scaffold + world view render correctness
_(covers the old P1.2 / P1.3 / P8.4.2 checklists)_

- [X] `cargo run` opens a 1200×800 window titled "Prime Watch 2" with a black background.
- [X] An egui window titled "Prime Watch" shows `Loaded 211 structs and 36 enums`; stdout prints
      the same line on startup.
- [X] Resizing the window does not panic and egui content is not stretched/garbled; closing the
      window exits cleanly (exit code 0).
- [X] The "World" panel shows 3D content (in the P1.3 spike: a rotating cube on a dark-blue clear,
      distinct from the black window background). Resizing the OS window / panel resizes the 3D
      content within ~1 frame with no stretching, garbling, aspect distortion, or panic.
- [X] Collapsing / shrinking the "World" panel to near-zero does not panic.
- [X] With `mem1.raw` loaded, the World view shows room collision geometry (grey/green/tinted tris
      with dark edge lines), white AABB wireframe boxes around loaded areas, and the coloured
      camera-frustum line set.
- [X] The translucent pink "last known non-colliding" box renders at the player position.
- [ ] Colours look right — the world view is **not** double-gamma'd (too dark / washed out) vs the
      egui chrome. (P8.2 applies `linear_to_srgb` in the world shader for the linear `Rgba8Unorm`
      egui composite target; confirm the composite does not double-encode.)
- [X] Cube / mesh winding is not inside-out (winding vs `cull_mode` never analytically verified).

---

## Manual verification — needs a live Dolphin + Metroid Prime 1

### P2.3 — Memory-access attach against a live Dolphin — `BLOCKED (needs user + live Dolphin)`

**POSIX (Linux/macOS):**
- [ ] With MP1 running in Dolphin: `get_dolphin_pids()` returns its pid; `attach_to_process(pid)`
      returns `true`.
- [X] `dolphin_memcpy(&mut buf, 0, 0x1800000)` fills a `0x1800000`-byte buffer; `&buf[0..6] ==
      b"GM8E01"` (matches `../primewatch2/mem1.raw` first bytes) and a live field (e.g. the
      `g_stateManager` chain) reads sanely.
- [X] Dropping / re-attaching does not leak (check `/proc/<our-pid>/maps` shrinks after
      `detach_from_process`).

**Windows:**
- [ ] `get_dolphin_pids()` returns Dolphin's pid; `attach_to_process(pid)` returns `true` and logs
      a "Found ram start" line.
- [ ] `dolphin_memcpy(&mut buf, 0, 0x1800000)` fills the buffer; `&buf[0..6] == b"GM8E01"`.
- [ ] Closing the game mid-session: the next `dolphin_memcpy` returns `false` and a later re-attach
      recovers without a leak (Task Manager handle count stable).
- [ ] Linux/macOS behaviour is unchanged (POSIX path untouched).

### D3 — Camera modes, menus, and Camera Controls window
_(old P8.4.2 / P8.4.6 checklists)_

- [X] `CameraMode::GameCam` matches the in-game camera; `FollowPlayer` frames the player (subject
      to D1).
- [X] Menu bar renders at the top; Culling / Camera / Triggers / Actors menus open and their items
      toggle.
- [X] Selecting a Culling item visibly changes collision-mesh face culling.
- [X] Camera → Follow Player shows Top/Center/Bottom; Detached shows the Speed slider + "Show camera
      controls"; toggling the latter opens/closes the Camera Controls window.
- [X] Camera Controls Yaw/Pitch drags read out in degrees and move the detached camera correctly;
      yaw wraps at ±360, pitch clamps at ±89.
- [ ] Camera orbit/zoom responds to input (arrow keys all modes, WASD/QE in Detached).
- [X] Trigger / Actor checkboxes change which entities draw in the world view.

### D4 — Entity rendering
_(old P8.4.3 / P8.4.4 / P8.4.5 checklists)_

- [ ] On-screen entity boxes / lines render for player, triggers, docks, actors, physics actors.
- [ ] Player cube/sphere + speed indicator render; trigger / dock / actor colours look right.
- [ ] Projectile / bomb / power-bomb / AI / pickup / chozo-ghost / collision-actor geometry renders
      in the right place.
- [ ] Bomb / AI / pickup text overlays land on-screen near their entities (HP / item / fuse-frame
      labels via the P9.1 overlay painter).
- [ ] WorldStatus window populates with sane area / chain / phase rows and the loading-resource
      list; PlayerStatus window shows sane player pos / vel / look-direction.

### D5 — Inspector tree + object table + watch windows
_(old P7.1 / P9.1 / P9.2 checklists)_

- [ ] A `CStateManager` tree in a watch window expands; primitive leaves show `{dec}/{hex}`; enums
      resolve names; `rstl::vector` shows `size/max size` + a working index spinner; click-to-copy
      puts the label on the clipboard.
- [ ] "Objects" window shows a non-zero count; "List of types" lists vtables with sane
      active/inactive splits; clicking an address row copies `{0x...., ""},`.
- [ ] "Copy unknowns (N)" grows N only as new unknown vtables appear; copies the block.
- [ ] Filter box: `@CPlayer` narrows to the player row; `-Effect` excludes; empty shows all
      (respecting "Show active only").
- [ ] Clicking an entity row opens a watch window titled `<Type> <eid hex>`; it tracks the entity
      across frames (via `last_known_uid`, then `eid`), shows "Not loaded" when the entity despawns,
      and closes cleanly via the titlebar X (no panic when several close at once).
- [ ] Hovering an entity row / having a watch window open highlights that entity in the 3D view
      (one-frame lag not perceptible).

### D6 — P9.1 items still untested (had a partial pass 2026-09-01)

- [ ] Shift+1–5 record player ghosts and Ctrl+1–5 clear them.
- [ ] "Reload Definitions" / the NOT-LOADED "Reload" button reload `prime_defs/` and update the
      status text.
- [X] Live-Dolphin attach path works end-to-end (only the `.raw` load path is verified).
- [ ] Mouse capture grabs/hides the cursor only on the button-down→capture transition, not every
      frame.

**Known non-blocking gaps from P9.1 (polish, not verification):**
1. Camera Controls window has no titlebar-X `.open()` binding — dismiss via the Camera menu toggle.
2. Scroll-to-zoom is dead while the pointer merely hovers the "World" egui window
   (`egui_wants_pointer_input()` is global; gate on the World `Image` response instead).
3. `rfd 0.15` pulls a heavy `zbus`/`ashpd` async tree and `LoadFromFile` blocks the event loop —
   revisit feature-trimming / the async API (see D7).

---

## Manual verification — needs a push to GitHub Actions

### P10.1 — Cross-platform CI + release packaging
- [ ] Push a branch / `v*` tag and watch Actions: all three legs (windows-latest / ubuntu-latest /
      macos-latest) build + test green.
- [ ] The Linux `apt-get` list in `rust_build.yml` is sufficient for a full `cargo build --release`
      on a clean runner (nothing else wgpu/rfd needs at compile time).
- [ ] `primewatch3-windows.zip` produced on the runner is a real zip; `primewatch3-macos.tar.gz`
      and `primewatch3-linux.tar.gz` unpack to `primewatch3/prime_defs/` + the binary.
- [ ] On a `v*` tag the single GitHub Release ends up with all three archives attached.

### D7 — Pre-release cleanup (optional, do before tagging a release)
- [ ] Trim `rfd` features / move to its async API so `LoadFromFile` no longer blocks the event loop.
- [ ] Consider renaming the crate `primewatch3` → `primewatch` (low priority, per CLAUDE.md).
