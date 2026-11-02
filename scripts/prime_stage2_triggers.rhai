let boss = get_entity_by_editor_id(0x00B0082);
if boss == () || !boss.extends("CMetroidPrimeStage2") {
    return;
}

let w = inspector_window("Prime Stage 2 triggers");

let has_event = boss.hasEventStarted.read_bool();
let can_attack = boss.canAttack.read_bool();
let hp = boss.healthInfo.health.read_f32();
let hp_threshold = boss.hpThreshold.read_f32();
let counter = boss.attackCounter.read_i32();
let next_threshold = boss.nextAttackThreshold.read_i32();

if has_event == () || can_attack == () || hp == () || hp_threshold == ()
    || counter == () || next_threshold == () {
    w.add("boss memory unreadable");
    show(w);
    return;
}

let player = get_player_entity();

// InMaxRange / TooClose: squared distance to the player vs. the attack range.
let boss_xf = boss.transform.read_transform();
let player_xf = if player != () { player.transform.read_transform() } else { () };
let min_range = boss.minAttackRange.read_f32();
let max_range = boss.maxAttackRange.read_f32();
if boss_xf != () && player_xf != () && min_range != () && max_range != () {
    let dist = boss_xf.translation().distance(player_xf.translation());
    w.add(`InMaxRange: ${dist < max_range} (dist ${dist} vs max ${max_range})`);
    w.add(`TooClose: ${dist < min_range} (dist ${dist} vs min ${min_range})`);
} else {
    w.add("InMaxRange / TooClose: ? (transform or range unreadable)");
}

// InPosition: boss z - player z > 0.25
let boss_z = boss.transform.posZ.read_f32();
let player_z = if player != () { player.transform.posZ.read_f32() } else { () };
if boss_z != () && player_z != () {
    let dz = boss_z - player_z;
    w.add(`InPosition: ${dz > 0.25} (boss z - player z = ${dz})`);
} else {
    w.add("InPosition: ? (boss/player transform unreadable)");
}

w.add(`ShouldMove: ${!has_event} (!hasEventStarted)`);

// Does not mutate here; the real call would also bump attackCounter when false.
if counter < next_threshold {
    w.add(`ShouldCrouch: false (attackCounter ${counter} < threshold ${next_threshold})`);
} else {
    w.add(`ShouldCrouch: true (attackCounter ${counter} >= threshold ${next_threshold})`);
}

let attack = if has_event { can_attack } else { true };
let attack_why = if has_event { "event started: canAttack" } else { "event not started" };
w.add(`ShouldAttack: ${attack} (${attack_why})`);

// The 50% roll uses the game's RNG, so only the gate is deterministic.
if hp > hp_threshold {
    w.add(`ShouldTaunt: 50% (hp ${hp} > threshold ${hp_threshold}, then RNG % 100 < 50)`);
} else {
    w.add(`ShouldTaunt: false (hp ${hp} <= threshold ${hp_threshold})`);
}

show(w);
