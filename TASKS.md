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

- [ ] Colours look right — the world view is **not** double-gamma'd (too dark / washed out) vs the
      egui chrome. (P8.2 applies `linear_to_srgb` in the world shader for the linear `Rgba8Unorm`
      egui composite target; confirm the composite does not double-encode.)

---

## Manual verification — needs a live Dolphin + Metroid Prime 1

### P2.3 — Memory-access attach against a live Dolphin — `BLOCKED (needs user + live Dolphin)`

**Windows:**
- [ ] `get_dolphin_pids()` returns Dolphin's pid; `attach_to_process(pid)` returns `true` and logs
      a "Found ram start" line.
- [ ] `dolphin_memcpy(&mut buf, 0, 0x1800000)` fills the buffer; `&buf[0..6] == b"GM8E01"`.
- [ ] Closing the game mid-session: the next `dolphin_memcpy` returns `false` and a later re-attach
      recovers without a leak (Task Manager handle count stable).
- [ ] Linux/macOS behaviour is unchanged (POSIX path untouched).

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

- [ ] "Reload Definitions" / the NOT-LOADED "Reload" button reload `prime_defs/` and update the
      status text.

**Known non-blocking gaps from P9.1 (polish, not verification):**
1. Camera Controls window has no titlebar-X `.open()` binding — dismiss via the Camera menu toggle.
2. `rfd 0.15` pulls a heavy `zbus`/`ashpd` async tree and `LoadFromFile` blocks the event loop —
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
