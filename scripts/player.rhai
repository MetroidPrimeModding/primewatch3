// Example PrimeWatch script.
//
// Scripts in ./scripts/*.rhai are re-run every frame. Build one or more
// windows with `inspector_window(title)`, fill them with `.add(...)`, and
// submit them with `show(w)`.
//
// Handle API:
//   get_player_entity()            -> the CPlayer handle (or () if unreadable)
//   get_entity_by_unique_id(id)    -> handle (or ())
//   get_entity_by_editor_id(id)    -> handle (or ())
//   get_state_manager(), get_main()
//   h["member"]  / h.member("x")   -> nested handle, auto-derefs pointers
//   h.elem(i)                      -> array element handle
//   h.address, h.type_name
//   h.extends("CActor")           -> bool
//   h.read_f32() / read_i32() / read_u32() / read_bool() / read_string() / ...
//                                 -> value, or () when the address is unreadable

let player = get_player_entity();

if player != () {
    let w = inspector_window("Player");

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

    show(w);
}
