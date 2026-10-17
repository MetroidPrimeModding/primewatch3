//   fwd speed   > 0 moving the way you face, < 0 backing up
//   right speed > 0 moving to your right,    < 0 to your left
//   offset      0 = moving straight ahead, +90 = straight right,
//               -90 = straight left, +-180 = straight back

fn round2(x) { (x * 100.0).round() / 100.0 }
fn round1(x) { (x * 10.0).round() / 10.0 }

let player = get_player_entity();
if player == () {
    return;
}

let vel = player.velocity.read_vec3();
let look = player.lookDir.read_vec3();
if vel == () || look == () {
    return;
}

let fwd = vec3(look.x, look.y, 0.0).normalize();
// Rotating forward 90 degrees clockwise (seen from above) gives right.
let right = vec3(fwd.y, -fwd.x, 0.0);
let vel_h = vec3(vel.x, vel.y, 0.0);

let fwd_speed = vel_h.dot(fwd);
let right_speed = vel_h.dot(right);
let speed = vel_h.length();

let w = inspector_window("Air Strafe");
w.anchor("right_bottom", -8.0, -8.0);

w.add(`fwd: ${round2(fwd_speed)}  right: ${round2(right_speed)}  speed: ${round2(speed)}`);
if speed < 0.001 || fwd.length_squared() == 0.0 {
    w.add("offset: -");
} else {
    // atan(a, b) is a.atan2(b): angle measured from forward toward right.
    let offset = to_degrees(atan(right_speed, fwd_speed));
    w.add(`offset: ${round1(offset)} deg`);
}

show(w);
