// Example PrimeWatch script.
//
// Scripts in ./scripts/*.rhai are re-run every frame. Build one or more
// windows with `inspector_window(title)`, fill them with `.add(...)`, and
// submit them with `show(w)`.
//
// Window API:
//   w.hide_title()                       -> hide the title bar (no dragging)
//   w.anchor(align, offset_x, offset_y)   -> pin to a screen corner/edge/center;
//                                            see scripts/player_status.rhai
//
// Handle API:
//   get_player_entity()            -> the CPlayer handle (or () if unreadable)
//   get_entity_by_unique_id(id)    -> handle (or ())
//   get_entity_by_editor_id(id)    -> handle (or ())
//   get_state_manager(), get_main()
//   h.member(name)
//   h["member"]  / h.member("x")   -> nested handle, auto-derefs pointers
//   h.elem(i)                      -> array element handle
//   h.address, h.type_name
//   h.extends("CActor")           -> bool
//   h.read_f32() / read_i32() / read_u32() / read_bool() / read_string() / ...
//                                 -> value, or () when the address is unreadable
//   h.read_vec3()   -> Vec3   from a CVector3f  (x, y, z)
//   h.read_quat()   -> Quat   from a CQuaternion (x, y, z, w)
//   h.read_mat4()   -> Mat4   from a CMatrix4f
//   h.read_transform() -> Mat4 from a CTransform (3x4, last column = translation)
//   (each read_* returns () when the address is unreadable)
//
// glam math (INT or FLOAT args both accepted):
//   vec3(x, y, z), vec3_splat(s), quat(x, y, z, w), quat_identity(),
//   quat_from_axis_angle(axis, radians), mat4_identity(),
//   mat4_from_translation(v) / _rotation(q) / _scale(v) /
//   _scale_rotation_translation(s, q, t)
//   Vec3:  + - * /, -v, scalar * / , ==, .x/.y/.z, .length(), .length_squared(),
//          .normalize(), .dot(o), .cross(o), .distance(o), .angle_between(o), .lerp(o, t)
//   Quat:  q * q, q * v (rotate), .x/.y/.z/.w, .normalize(), .inverse(),
//          .conjugate(), .length(), .dot(o), .slerp(o, t), .rotate_vec3(v)
//   Mat4:  m * m, m * v (transform_point3), .inverse(), .transpose(),
//          .determinant(), .transform_point3(v), .transform_vector3(v),
//          .translation(), .rotation(), .scale()

let player = get_player_entity();

if player != () {
    let w = inspector_window("Custom Window");

    // Live tree nodes — re-read from memory every frame by the inspector.
    w.add("sjTimer", player.sjTimer);
    // You can also use the indexer syntax
    w.add("morphState", player["morphState"]);
    w.add("transform", player.transform);
    w.add("pos X", player.transform.posX);
    w.add("pos Y", player.transform.posY);
    w.add("pos Z", player.transform.posZ);

    // A snapshot value line.
    let t = player["sjTimer"].read_f32();
    if t != () {
        w.add("sjTimer (this frame)", t);
    }

    // glam math: read the transform + velocity and do some vector work.
    let xf = player.transform.read_transform();
    let vel = player.velocity.read_vec3();
    if xf != () && vel != () {
        w.add("world pos", xf.translation());
        w.add("speed", vel.length());
        // Velocity direction in the player's local frame.
        w.add("local vel dir", xf.rotation().inverse() * vel.normalize());
    }

    // You can get an entity by its unique ID or editor ID.
    let e = get_entity_by_editor_id(0x00000083);
    if e != () {
        w.add("Trigger editor id", e.editorID);
        w.add("Trigger transform", e.transform);
    }

    show(w);
}
