# Conversion tasks — remaining work

The Rust port of PrimeWatch is **code-complete through Phase 10**. Every phase (P0–P10) has
shipped and been reviewed. What remains is **manual verification that cannot be done in this
environment** — it needs a display, a live Dolphin + Metroid Prime 1, and/or a push to GitHub
Actions — plus one open software decision.

The old per-task archives (`completed_tasks/`) have been removed; the shipped-state summaries
live in the git history (`port(PX.Y): …` commits on the `rust-conversion` branch).

Status legend: `BLOCKED (reason)` · `TODO`

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

**Known non-blocking gaps from P9.1 (polish, not verification):**
1. `rfd 0.15` pulls a heavy `zbus`/`ashpd` async tree and `LoadFromFile` blocks the event loop —
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
