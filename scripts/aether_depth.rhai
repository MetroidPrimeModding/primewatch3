// Aether depth predictor.
//
// The "aether" is anywhere the player's touch bounds overlap no *alive* area
// AABB (`CGroundMovement::CheckFalling`). On the first step that happens, the
// game zeroes vertical velocity and momentum and sends OnFloor, so the player
// stops falling and stands there. This script replays that step by step for a
// straight vertical fall from the current position and reports where it ends.
//
// Per step (`CPhysicsActor::PredictLinearMotion` + `AddMotionState`):
//   z  += dt * vz        (old velocity)
//   vz += dt * gravity   (momentum = gravity * mass, so mass cancels)
// then `CheckFalling` runs on the new position.
//
// Floating back out: `MoveGroundCollider_New` nudges the player up by 0.0005
// each step when nothing collides at the raised spot (always true in the
// aether), and the "undo" in the no-floor branch only edits a local copy of
// the transform. So the player rises 0.0005 per frame (f32, so the real step
// shrinks slightly as |z| grows). "Pop out" is the first step whose touch
// bounds overlap an alive area again; `CheckFalling` then sends Falling
// instead of OnFloor. Whether the player then stays or bobs back down at the
// edge isn't modelled. If no area lies above, the float never ends.
//
// Assumptions, all of which make the number approximate rather than exact:
//   - Fixed dt of 1/60 s and f32 rounding is ignored (rhai uses f64), so a
//     fall that lands within ~1e-4 of an area edge could differ by one step.
//   - Pure vertical fall: horizontal motion, geometry collision, jumps, and
//     fluid / dash gravity are ignored. Gravity is the tweak's normal value.
//   - Morph ball: not handled. The ball moves through `MoveAndCollide`, which
//     never calls `CheckFalling`, so it doesn't stop in the aether this way.
//   - Areas are taken as they are loaded now; streaming during the fall that
//     brings another area into range isn't modelled.

const DT = 1.0 / 60.0;
const MAX_STEPS = 1200;
const CHAIN_ALIVE = 3;

fn round3(x) { (x * 1000.0).round() / 1000.0 }

// Does a box [lo, hi] overlap [alo, ahi]? Touching counts (`DoBoundsOverlap`).
fn overlaps(lo, hi, alo, ahi) {
    !(alo.x > hi.x || lo.x > ahi.x
      || alo.y > hi.y || lo.y > ahi.y
      || alo.z > hi.z || lo.z > ahi.z)
}

let player = get_player_entity();
let sm = get_state_manager();
let tweak = get_tweak_player();
if player == () || sm == () || tweak == () {
    return;
}

let gravity = tweak.normalGravAccel.read_f32();
let xf = player.transform.read_transform();
let vel = player.velocity.read_vec3();
let morph = player.morphState.read_u32();
let world = sm.world;
if gravity == () || xf == () || vel == () || morph == () || world == () {
    return;
}

if morph != 0 {
    let w = inspector_window("Aether depth");
    w.add("unmorphed only (the ball doesn't stop in the aether)");
    show(w);
    return;
}

// Touch bounds relative to the player's origin.
let off = player.primitiveOffset.read_vec3();
let lo = player.baseBoundingBox.min.read_vec3();
let hi = player.baseBoundingBox.max.read_vec3();
if off == () || lo == () || hi == () { return; }
let local_lo = lo + off;
let local_hi = hi + off;

// Snapshot the alive areas once.
let area_lo = [];
let area_hi = [];
let areas = world.areas;
let count = areas.count.read_u32();
if count == () { return; }
let slots = areas.first;
for i in 0..count {
    let area = slots.elem(i).value;
    if area == () || area.curChain.read_i32() != CHAIN_ALIVE { continue; }
    let lo = area.aabb.min.read_vec3();
    let hi = area.aabb.max.read_vec3();
    if lo == () || hi == () { continue; }
    area_lo.push(lo);
    area_hi.push(hi);
}

let start = xf.translation();

// Returns true if the player at `pos` overlaps any alive area.
fn in_bounds(pos, local_lo, local_hi, area_lo, area_hi) {
    let lo = pos + local_lo;
    let hi = pos + local_hi;
    for i in 0..area_lo.len() {
        if overlaps(lo, hi, area_lo[i], area_hi[i]) { return true; }
    }
    false
}

// Gap between the player's top and the lowest overlapping-in-XY area bottom
// above it. () when no area sits above the player horizontally.
fn gap_to_area_above(pos, local_lo, local_hi, area_lo, area_hi) {
    let top = pos.z + local_hi.z;
    let best = ();
    for i in 0..area_lo.len() {
        let alo = area_lo[i];
        let ahi = area_hi[i];
        let x_ok = !(alo.x > pos.x + local_hi.x || pos.x + local_lo.x > ahi.x);
        let y_ok = !(alo.y > pos.y + local_hi.y || pos.y + local_lo.y > ahi.y);
        if x_ok && y_ok && alo.z > top {
            let gap = alo.z - top;
            if best == () || gap < best { best = gap; }
        }
    }
    best
}

// floor(log2(|x|)), x != 0.
fn exp2_of(x) {
    let l = x.abs().ln();
    (l / 0.6931471805599453).floor()
}

// f32 spacing at x.
fn ulp32(x) {
    if x == 0.0 { return 1.0e-12; }
    let e = exp2_of(x) - 23.0;
    2.0 ** e
}

// Next |z| power of two the value rises into (negative z) or past (positive z).
fn binade_boundary(z) {
    let e = exp2_of(z);
    if z < 0.0 {
        let p = 2.0 ** e;
        return 0.0 - p;
    }
    e += 1.0;
    2.0 ** e
}

// Steps of FLOAT_STEP (rounded to the f32 grid at the current |z|) from z
// until z >= target. -1 if never (stuck at huge |z|).
fn steps_to_reach(z, target, step) {
    let frames = 0;
    for iter in 0..80 {
        if z >= target { return frames; }
        let ulp = ulp32(z);
        let q = (step / ulp).round() * ulp;
        if q <= 0.0 { return -1; }
        // z's f32 grid gets finer when it rises into the next lower |z| binade
        // (negative z) and coarser when it crosses the next power of two (positive z).
        let limit = target;
        if z != 0.0 {
            let b = binade_boundary(z);
            if b < limit { limit = b; }
        }
        let n = ((limit - z) / q).ceiling().to_int();
        if n < 1 { n = 1; }
        frames += n;
        z += n * q;
    }
    -1
}

// Frames until the player at `pos` (resting, then rising) overlaps an alive area,
// or () if nothing lies above them.
fn pop_out_frames(pos, local_lo, local_hi, area_lo, area_hi) {
    let top = pos.z + local_hi.z;
    let target = ();
    for i in 0..area_lo.len() {
        let alo = area_lo[i];
        let ahi = area_hi[i];
        let x_ok = !(alo.x > pos.x + local_hi.x || pos.x + local_lo.x > ahi.x);
        let y_ok = !(alo.y > pos.y + local_hi.y || pos.y + local_lo.y > ahi.y);
        if x_ok && y_ok && alo.z > top {
            let t = alo.z - local_hi.z;
            if target == () || t < target { target = t; }
        }
    }
    if target == () { return (); }
    steps_to_reach(pos.z, target, 0.0005)
}

fn describe_pop(frames) {
    if frames == () { return "pop out: never (no area above)"; }
    if frames < 0 { return "pop out: never (float step lost to f32 rounding)"; }
    let secs = round3(frames.to_float() / 60.0);
    "pop out in " + frames + " frames (" + secs + "s)"
}

let w = inspector_window("Aether depth");

if area_lo.len() == 0 {
    w.add("no alive areas readable");
    show(w);
    return;
}

if !in_bounds(start, local_lo, local_hi, area_lo, area_hi) {
    let gap = gap_to_area_above(start, local_lo, local_hi, area_lo, area_hi);
    w.add("already in the aether (not falling)");
    if gap == () {
        w.add("depth: n/a (no area directly above)");
    } else {
        w.add(`depth: ${round3(gap)} below the area above`);
    }
    w.add(describe_pop(pop_out_frames(start, local_lo, local_hi, area_lo, area_hi)));
    show(w);
    return;
}

let pos = start;
let vz = vel.z;
let steps = 0;
let found = false;
while steps < MAX_STEPS {
    pos = vec3(pos.x, pos.y, pos.z + DT * vz);
    vz += DT * gravity;
    steps += 1;
    if !in_bounds(pos, local_lo, local_hi, area_lo, area_hi) {
        found = true;
        break;
    }
}

if !found {
    w.add(`stays in bounds for ${MAX_STEPS} steps (${round3(MAX_STEPS * DT)}s)`);
    show(w);
    return;
}

let depth = gap_to_area_above(pos, local_lo, local_hi, area_lo, area_hi);
w.add(`aether depth: ${if depth == () { "n/a" } else { round3(depth).to_string() }}`);
w.add(`rest z: ${round3(pos.z)} (fall ${round3(start.z - pos.z)})`);
w.add(`enters aether in ${steps} steps (${round3(steps * DT)}s), vz ${round3(vz)}`);
w.add(describe_pop(pop_out_frames(pos, local_lo, local_hi, area_lo, area_hi)));
show(w);
