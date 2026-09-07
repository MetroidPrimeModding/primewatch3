// Player status HUD — a script port of the old hardcoded "PlayerStatus"
// window (pos / vel / look readout), pinned to the bottom-left corner with
// no title bar, same as the window it replaces.
//
// Demonstrates the window placement API:
//   w.hide_title()                       -> no title bar (and no dragging)
//   w.anchor(align, offset_x, offset_y)  -> pin to a screen corner/edge/center;
//                                            align is "left"/"center"/"right"
//                                            joined with "_top"/"_center"/"_bottom",
//                                            e.g. "left_bottom", "right_top".
//                                            offsets are in points, same sign
//                                            convention as the corner (a
//                                            "_bottom" offset of -8.0 sits 8
//                                            points above the screen edge).
//
// ...and the glam math helpers: one `.read_transform()` / `.read_vec3()` per
// struct instead of three scalar `.read_f32()` calls, then `Vec3` arithmetic.

fn round3(x) { (x * 1000.0).round() / 1000.0 }
fn round1(x) { (x * 10.0).round() / 10.0 }

let player = get_player_entity();
if player == () {
    return;
}

// One read each: CTransform -> Mat4, CVector3f -> Vec3.
let xf = player.transform.read_transform();
let vel = player.velocity.read_vec3();
let look = player.lookDir.read_vec3();
if xf == () || vel == () || look == () {
    return;
}

let pos = xf.translation();

// Project velocity / look onto the horizontal (x/y) plane.
let vel_h = vec3(vel.x, vel.y, 0.0);
let look_h = vec3(look.x, look.y, 0.0);
let hvel_h = vel_h.length();

// `.normalize()` returns the zero vector when the input is degenerate, matching
// the old `if len > 0 { v / len } else { 0 }` guards.
let fwd = look_h.normalize();
let vdir = vel_h.normalize();

// `atan(a, b)` computes `a.atan2(b)` (Rhai's two-arg overload).
let forward_deg = to_degrees(atan(fwd.y, fwd.x));
let vel_deg = to_degrees(atan(vdir.y, vdir.x));
let delta_deg = forward_deg - vel_deg;

let w = inspector_window("PlayerStatus");
w.hide_title();
w.anchor("left_bottom", 8.0, -8.0);

w.add(`pos: ${round3(pos.x)}x ${round3(pos.y)}y ${round3(pos.z)}z`);
w.add(`vel: ${round3(vel.x)}x ${round3(vel.y)}y ${round3(vel.z)}z ${round3(hvel_h)}h`);
w.add(
    `look: ${round3(fwd.x)}x ${round3(fwd.y)}y ${round1(forward_deg)}deg | ` +
    `vel ${round3(vdir.x)}x ${round3(vdir.y)}y ${round1(vel_deg)}deg | ${round1(delta_deg)} deg`
);

show(w);
