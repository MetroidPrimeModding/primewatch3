let player = get_player_entity();

if player == () {
    return;
};

let ball = player.morphBall;

if ball == () {
    return;
}

let w = inspector_window("Morph lift");

let liftControlCount = ball.liftControlForceAvgCount.read_u32();
let liftControlAvg = vec3(0.0, 0.0, 0.0);
for i in 0..liftControlCount {
    liftControlAvg += ball.liftControlForceAvg[i].read_vec3();
}
if liftControlCount > 0 {
    liftControlAvg /= liftControlCount;
}
let lift_mag = liftControlAvg.length();

let liftSpeedCount = ball.liftSpeedAvgCount.read_u32();
let liftSpeedAvg = 0.0;
for i in 0..liftSpeedCount {
    liftSpeedAvg += ball.liftSpeedAvg[i].read_f32();
}
if (liftSpeedCount >= 0) {
    liftSpeedAvg /= liftSpeedCount;
}

w.add("lift force", lift_mag.round());
w.add("lift speed", liftSpeedAvg.round());


show(w);

