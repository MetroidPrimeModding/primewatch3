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

fn round3(x) { (x * 1000.0).round() / 1000.0 }
fn round1(x) { (x * 10.0).round() / 10.0 }

let player = get_player_entity();
if player == () {
    return;
}

let px = player.transform.posX.read_f32();
let py = player.transform.posY.read_f32();
let pz = player.transform.posZ.read_f32();

let vx = player.velocity.x.read_f32();
let vy = player.velocity.y.read_f32();
let vz = player.velocity.z.read_f32();

let lx = player.lookDir.x.read_f32();
let ly = player.lookDir.y.read_f32();

if px == () || py == () || pz == () || vx == () || vy == () || vz == () || lx == () || ly == () {
    return;
}

let hvel_h = hypot(vx, vy);

let flen = hypot(lx, ly);
let fx = if flen > 0.0 { lx / flen } else { 0.0 };
let fy = if flen > 0.0 { ly / flen } else { 0.0 };

let velx = if hvel_h > 0.0 { vx / hvel_h } else { 0.0 };
let vely = if hvel_h > 0.0 { vy / hvel_h } else { 0.0 };

// `atan(a, b)` computes `a.atan2(b)` (Rhai's two-arg overload).
let forward_deg = to_degrees(atan(fy, fx));
let vel_deg = to_degrees(atan(vely, velx));
let delta_deg = forward_deg - vel_deg;

let w = inspector_window("PlayerStatus");
w.hide_title();
w.anchor("left_bottom", 8.0, -8.0);

w.add(`pos: ${round3(px)}x ${round3(py)}y ${round3(pz)}z`);
w.add(`vel: ${round3(vx)}x ${round3(vy)}y ${round3(vz)}z ${round3(hvel_h)}h`);
w.add(
    `look: ${round3(fx)}x ${round3(fy)}y ${round1(forward_deg)}deg | ` +
    `vel ${round3(velx)}x ${round3(vely)}y ${round1(vel_deg)}deg | ${round1(delta_deg)} deg`
);

show(w);
