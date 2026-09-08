//! Predicts `CBallCamera::CheckFailsafeFromMorphBallState`
//! (metaforce `Runtime/Camera/CBallCamera.cpp:1547`) against the live static
//! collision world.
//!
//! The game runs this the instant you unmorph. `CBallCamera::TransitionFromMorphBallState`
//! (`CBallCamera.cpp:2005`) builds a 4-point Bézier "pull-back" spline from the
//! current camera position out to the player's eye, then
//! `CheckFailsafeFromMorphBallState` samples that spline in 6 segments and casts
//! a ray forward and backward along each. If any segment's ray-entry and
//! ray-exit points are more than `0.3` apart — i.e. the spline passes through a
//! meaningful slab of solid geometry — it returns `false`, and the caller
//! (`CPlayer::TransitionFromMorphBallState`, `CPlayer.cpp:460`) snaps the
//! transition filter and calls `LeaveMorphBallState` immediately instead of
//! playing the cinematic camera dolly.
//!
//! **"The failsafe will trigger" == this function returns `false`.**
//! [`FailsafePrediction::would_trigger`] is that inverted sense.
//!
//! ## Why the ray tracer is enough
//! `CheckFailsafeFromMorphBallState` passes an **empty** `nearList` to
//! `RayWorldIntersection`, so its dynamic-actor half (`RayDynamicIntersection`)
//! iterates nothing and the call collapses to `RayStaticIntersection` — exactly
//! the brute-force static cast in [`crate::world::ray_trace`]. No dynamic-actor
//! schema or collision-primitive work is needed here.
//!
//! ## Deviation: `CheckTransitionLineOfSight` (`CBallCamera.cpp:1967`)
//! The real code pulls the middle spline control point in toward the eye when a
//! **moving `0.6` sphere** swept from `eyePos` to `behindPos` hits world
//! geometry (`CGameCollision::DetectCollision_Cached_Moving`, plus an area
//! collision cache and collider list). We approximate that sweep with a single
//! ray `eyePos -> behindPos` through the same `BallCameraFilter` and use the hit
//! distance as the occlusion distance ([`FailsafeInputs`] feeds the endpoints;
//! [`predict_failsafe`] does the cast).
//!
//! This reproduces the *behaviour* ("behind point gets pulled in when something
//! is in the way") but is not sphere-accurate: a thin obstacle just off the ray
//! line is missed, so in a tight spot the predicted middle control point can sit
//! `~0.6` further back than the game's, and the prediction can read "clear"
//! where the game trips (or, less often, the reverse). If predictions turn out
//! to drift in practice, the correct fix is a swept-sphere-vs-mesh test — expand
//! each candidate triangle by the sphere radius (Minkowski sum) or sphere-cast
//! against the collision mesh — feeding `eye_to_occ_dist`.

use glam::Vec3;

use crate::ctx::Ctx;
use crate::mem::globals::{get_state_manager, get_tweak_player};
use crate::mem::math_utils::{read_as_transform, read_as_vec3};
use crate::structs::prime_structs::GameInstance;
use crate::world::collision_mesh::{CollisionMesh, ECollisionMaterial};
use crate::world::ray_trace::{MaterialFilter, Ray, raycast_world};

fn read_vec3_member(ctx: &Ctx, parent: &GameInstance, name: &str) -> Option<Vec3> {
  read_as_vec3(ctx, &parent.get_member(ctx, name)?)
}

/// `BallCameraFilter` (`CBallCamera.cpp:262`,
/// `MakeIncludeExclude({Solid}, {ProjectilePassthrough, Player, Character, CameraPassthrough})`)
/// restricted to static triangles: `Player` / `Character` are actor-only
/// material bits and are never set on world geometry, so for a static cast the
/// filter is "is `Solid`, and is neither shoot-through nor camera-through".
pub fn ball_camera_filter(m: ECollisionMaterial) -> bool {
  m.contains(ECollisionMaterial::SOLID)
    && !m.contains(ECollisionMaterial::SHOOT_THRU) // EMaterialTypes::ProjectilePassthrough
    && !m.contains(ECollisionMaterial::CAMERA_THRU) // EMaterialTypes::CameraPassthrough
}

/// `zeus::getBezierPoint` (`extern/zeus/src/Math.cpp:162`) — cubic Bézier over
/// four control points via De Casteljau.
fn bezier_point(a: Vec3, b: Vec3, c: Vec3, d: Vec3, t: f32) -> Vec3 {
  let omt = 1.0 - t;
  let ab = a * omt + b * t;
  let bc = b * omt + c * t;
  let cd = c * omt + d * t;
  let q0 = ab * omt + bc * t;
  let q1 = bc * omt + cd * t;
  q0 * omt + q1 * t
}

/// `CBallCamera::GetFailsafeSplinePoint` (`CBallCamera.cpp:1537`) for the
/// 4-control-point case the morph-ball transition always uses: `points.size() - 3
/// == 1`, so `t` is not remapped and `baseIdx` stays `0`, leaving a plain cubic
/// Bézier over all four points. `t` is passed unclamped by the game but only
/// ever in `[0, 1]` here.
pub fn failsafe_spline_point(p: &[Vec3; 4], t: f32) -> Vec3 {
  bezier_point(p[0], p[1], p[2], p[3], t)
}

/// The live state `CheckFailsafeFromMorphBallState` needs, already resolved to
/// world space. Pulled from memory by [`predict_failsafe_from_live`].
#[derive(Clone, Copy, Debug)]
pub struct FailsafeInputs {
  /// `x30_camXf.origin` — the ball camera's current translation.
  pub cam_origin: Vec3,
  /// `x1d8_lookPos` on the ball camera.
  pub look_pos: Vec3,
  /// The direction the player will face on unmorph — `x518_leaveMorphDir`,
  /// flattened + normalised. **Not** the player transform's `GetForward()`,
  /// which rolls with the ball while morphed. See [`predict_failsafe_from_live`].
  pub player_forward: Vec3,
  /// `CPlayer::GetEyePosition()` — player translation plus eye height on `Z`.
  pub eye_pos: Vec3,
}

/// Result of the prediction.
#[derive(Clone, Debug)]
pub struct FailsafePrediction {
  /// The game's `CheckFailsafeFromMorphBallState` would return `false` — the
  /// cinematic unmorph camera move is skipped ("the failsafe fires").
  pub would_trigger: bool,
  /// Echo of the inputs the spline was built from (for diagnostics).
  pub inputs: FailsafeInputs,
  /// `|x60_lookPos - x30_camXf.origin|` — feeds `behindPos` (`= forward * -0.6 *
  /// look_dist + eye_pos`). A wild value here means a bad `lookPos` /
  /// `cam_origin` read.
  pub look_dist: f32,
  /// The reconstructed pull-back spline's 4 Bézier control points:
  /// `[camXf.origin, behindPos, behindPos, eyePos]`.
  pub spline_points: [Vec3; 4],
  /// Largest ray entry/exit separation over the 6 sampled segments — the value
  /// compared against the game's `0.3` threshold.
  pub worst_separation: f32,
  /// How many of the 6 segments exceeded `0.3`.
  pub obstructed_segments: u8,
  /// The line-of-sight approximation pulled the middle control point in toward
  /// the eye (something is between the eye and the ideal behind-point). See the
  /// module-level deviation note.
  pub behind_point_occluded: bool,
}

impl FailsafePrediction {
  /// Sample the reconstructed spline into an `n`-segment polyline (`t` in
  /// `[0, 1]`) for an overlay.
  pub fn spline_polyline(&self, n: usize) -> Vec<Vec3> {
    let n = n.max(1);
    (0..=n)
      .map(|i| failsafe_spline_point(&self.spline_points, i as f32 / n as f32))
      .collect()
  }
}

/// The number of segments the spline is sampled into (`curT < 6.f` in the C++).
const SEGMENTS: u32 = 6;
/// `CheckTransitionLineOfSight`'s `colRadius` (`CBallCamera.cpp:2014`) — kept for
/// the behind-point placement math, not (yet) as a sphere radius.
const LOS_COL_RADIUS: f32 = 0.6;

/// Pure core: reconstruct the spline from [`FailsafeInputs`] and run the
/// 6-segment sweep against `meshes`. `meshes` is iterated once per ray cast, so
/// it must be cheaply `Clone` (e.g. `HashMap::values()`).
pub fn predict_failsafe<'a>(
  inp: &FailsafeInputs,
  meshes: impl IntoIterator<Item = &'a CollisionMesh> + Clone,
) -> FailsafePrediction {
  let filter: MaterialFilter = &ball_camera_filter;

  // --- reconstruct the spline (CBallCamera::TransitionFromMorphBallState :2010-2024) ---
  let look_dist = (inp.look_pos - inp.cam_origin).length();
  let behind_pos = inp.player_forward * (LOS_COL_RADIUS * -look_dist) + inp.eye_pos;

  // CheckTransitionLineOfSight (:1967), approximated by a single ray — see module docs.
  let eye_to_behind = behind_pos - inp.eye_pos;
  let mag = eye_to_behind.length();
  let (mid, behind_point_occluded) = if mag > 1.0e-6 {
    let dir = eye_to_behind / mag;
    match raycast_world(
      meshes.clone(),
      Ray {
        origin: inp.eye_pos,
        dir,
      },
      mag,
      filter,
    ) {
      // x6c_behindPos = playerXf.GetForward() * -eyeToOccDist + eyePos
      Some(hit) => (inp.player_forward * -hit.t + inp.eye_pos, true),
      None => (behind_pos, false),
    }
  } else {
    (behind_pos, false)
  };

  let pts = [inp.cam_origin, mid, mid, inp.eye_pos];

  // --- the 6-segment sweep (CheckFailsafeFromMorphBallState :1553-1580) ---
  let mut worst_separation = 0.0_f32;
  let mut obstructed_segments = 0u8;

  for i in 0..SEGMENTS {
    let point_a = failsafe_spline_point(&pts, i as f32 / SEGMENTS as f32);
    let point_b = failsafe_spline_point(&pts, (i as f32 + 1.0) / SEGMENTS as f32);
    let point_delta = point_b - point_a;
    let delta_mag = point_delta.length();
    if delta_mag <= 0.1 {
      // resultsA/B push a default (invalid) CRayCastResult -> skipped below.
      continue;
    }
    let dir = point_delta / delta_mag;
    let res_a = raycast_world(
      meshes.clone(),
      Ray {
        origin: point_a,
        dir,
      },
      delta_mag,
      filter,
    );
    let res_b = raycast_world(
      meshes.clone(),
      Ray {
        origin: point_b,
        dir: -dir,
      },
      delta_mag,
      filter,
    );

    let Some(res_a) = res_a else { continue };

    // The C++ reads `resB.GetPoint()` unconditionally; a default / invalid
    // CRayCastResult has `point == (0,0,0)`, so an invalid `res_b` produces a
    // separation of `|res_a.point|` (huge, in world coords) and trips the
    // failsafe. Faithful port of that quirk.
    let b_point = res_b.map(|h| h.point).unwrap_or(Vec3::ZERO);
    let mut separation = res_a.point - b_point;
    if separation.length() < 0.00001 {
      separation = failsafe_spline_point(&pts, (1.0 + i as f32) / SEGMENTS as f32) - res_a.point;
    }
    let sep_mag = separation.length();
    worst_separation = worst_separation.max(sep_mag);
    if sep_mag > 0.3 {
      obstructed_segments += 1;
    }
  }

  FailsafePrediction {
    would_trigger: obstructed_segments > 0,
    inputs: *inp,
    look_dist,
    spline_points: pts,
    worst_separation,
    obstructed_segments,
    behind_point_occluded,
  }
}

/// Read the live camera / player state and run [`predict_failsafe`].
///
/// Everything comes from memory here: the ball camera transform + `lookPos`, the
/// player transform (for `GetTranslation()`) and the player's unmorph facing
/// direction + eye height. Returns `None` if any required read fails (e.g. no
/// `CBallCamera` yet, schema miss).
///
/// **`player_forward` is `x518_leaveMorphDir`, not the player transform basis.**
/// While morphed the player's `x34_transform` *rolls with the ball*, so its
/// `GetForward()` tumbles as you move. The game reconstructs the spline in
/// `CBallCamera::TransitionFromMorphBallState` only *after*
/// `CPlayer::LeaveMorphBallState` has re-set the transform to
/// `LookAt(pos, pos + f31)` where `f31 == x518_leaveMorphDir` (`CPlayer.cpp:397,
/// 420`) — a flattened, normalised travel direction maintained by
/// `CalculatePlayerMovementDirection`. So we use that direction directly.
/// Fallbacks: `x50c_moveDir`, then flattened `player_pos - cam_origin`
/// (`camToPlayer`, `CPlayer.cpp:392`).
pub fn predict_failsafe_from_live<'a>(
  ctx: &Ctx,
  meshes: impl IntoIterator<Item = &'a CollisionMesh> + Clone,
) -> Option<FailsafePrediction> {
  let sm = get_state_manager();
  let cam_mgr = sm.get_member(ctx, "cameraManager")?;
  let ball_cam = cam_mgr.get_member(ctx, "ballCamera")?; // auto-derefs *CBallCamera

  let cam_origin = read_as_transform(ctx, &ball_cam.get_member(ctx, "transform")?)?
    .w_axis
    .truncate();
  let look_pos = read_as_vec3(ctx, &ball_cam.get_member(ctx, "lookPos")?)?;

  let player = sm.get_member(ctx, "player")?;
  let player_xf = read_as_transform(ctx, &player.get_member(ctx, "transform")?)?;
  let player_pos = player_xf.w_axis.truncate();

  // Flatten to the horizontal plane + normalise; `None` if degenerate.
  let flat_dir = |v: Vec3| Vec3::new(v.x, v.y, 0.0).try_normalize();
  let player_forward = read_vec3_member(ctx, &player, "leaveMorphDir")
    .and_then(flat_dir)
    .or_else(|| read_vec3_member(ctx, &player, "moveDir").and_then(flat_dir))
    .or_else(|| flat_dir(player_pos - cam_origin))?;

  let eye_height = player_eye_height(ctx, &player)?;
  let eye_pos = player_pos + Vec3::new(0.0, 0.0, eye_height);

  Some(predict_failsafe(
    &FailsafeInputs {
      cam_origin,
      look_pos,
      player_forward,
      eye_pos,
    },
    meshes,
  ))
}

/// `CPlayer::GetEyeHeight` (`CPlayer.cpp:5084`):
/// `x9c8_eyeZBias + (x2d8_fpBounds.max.z - g_tweakPlayer->GetEyeOffset())`.
fn player_eye_height(ctx: &Ctx, player: &GameInstance) -> Option<f32> {
  let eye_z_bias = player.get_member(ctx, "eyeZBias")?.read_f32(ctx)?;
  let fp_bounds_max_z = player
    .get_member(ctx, "fpBounds")?
    .get_member(ctx, "max")?
    .get_member(ctx, "z")?
    .read_f32(ctx)?;
  let eye_offset = get_tweak_player(ctx)?
    .get_member(ctx, "eyeOffset")?
    .read_f32(ctx)?;
  Some(eye_z_bias + (fp_bounds_max_z - eye_offset))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::world::collision_mesh::CollisionMesh;

  /// Diagnostic (not an assertion): dump every input the failsafe prediction
  /// derives from the live `mem1.raw`, so a bad struct offset is obvious.
  /// `cargo test dump_failsafe_inputs_from_live -- --nocapture --ignored`
  #[test]
  #[ignore = "diagnostic dump; run explicitly with --nocapture"]
  fn dump_failsafe_inputs_from_live() {
    use crate::mem::game_memory::GameMemory;
    use crate::mem::globals::get_state_manager;
    use crate::mem::math_utils::{read_as_quat, read_as_transform, read_as_vec3};
    use crate::structs::prime_structs::GameStructs;

    let path = std::env::var("PRIMEWATCH_MEM1_RAW")
      .unwrap_or_else(|_| format!("{}/mem1.raw", env!("CARGO_MANIFEST_DIR")));
    if !std::path::Path::new(&path).exists() {
      eprintln!("skipping: {path} not found");
      return;
    }
    let mut mem = GameMemory::new();
    mem.load_from_file(&path).expect("read mem1.raw");
    let mut structs = GameStructs::new_empty();
    structs
      .load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/prime_defs"))
      .expect("load prime_defs");
    let ctx = Ctx::new(&structs, &mem);

    let sm = get_state_manager();
    let player = sm.get_member(&ctx, "player").expect("player");
    let player_addr = player.address;
    eprintln!("player @ {player_addr:#010x}");

    let tf = read_as_transform(&ctx, &player.get_member(&ctx, "transform").unwrap()).unwrap();
    eprintln!("player.transform translation = {:?}", tf.w_axis.truncate());
    eprintln!(
      "player.transform basis: x_axis(left)={:?}  y_axis(FWD)={:?}  z_axis(up)={:?}",
      tf.x_axis.truncate(),
      tf.y_axis.truncate(),
      tf.z_axis.truncate()
    );

    let q = read_as_quat(&ctx, &player.get_member(&ctx, "orientation").unwrap()).unwrap();
    eprintln!("player.orientation = {q:?}  len={}", q.length());
    eprintln!("  q * +Y = {:?}   (should match basis y_axis)", q * Vec3::Y);

    for name in ["lookDir", "moveDir", "leaveMorphDir"] {
      eprintln!(
        "player.{name} = {:?}",
        read_vec3_member(&ctx, &player, name)
      );
    }
    eprintln!(
      "player.morphState = {:?}",
      player
        .get_member(&ctx, "morphState")
        .and_then(|m| m.read_u32(&ctx))
    );

    // raw 4 floats at orientation offset
    let oaddr = player.get_member(&ctx, "orientation").unwrap().address;
    let f = |o: u32| mem.read_f32(oaddr + o).unwrap_or(f32::NAN);
    eprintln!(
      "  raw @orientation: [+0]={:.4} [+4]={:.4} [+8]={:.4} [+C]={:.4}",
      f(0),
      f(4),
      f(8),
      f(0xC)
    );

    // eye height components
    let eye_z_bias = player
      .get_member(&ctx, "eyeZBias")
      .and_then(|m| m.read_f32(&ctx));
    let fp_max = player
      .get_member(&ctx, "fpBounds")
      .and_then(|m| m.get_member(&ctx, "max"))
      .and_then(|m| read_as_vec3(&ctx, &m));
    let fp_min = player
      .get_member(&ctx, "fpBounds")
      .and_then(|m| m.get_member(&ctx, "min"))
      .and_then(|m| read_as_vec3(&ctx, &m));
    let eye_off = get_tweak_player(&ctx)
      .and_then(|t| t.get_member(&ctx, "eyeOffset"))
      .and_then(|m| m.read_f32(&ctx));
    eprintln!(
      "eyeZBias={eye_z_bias:?}  fpBounds.min={fp_min:?}  fpBounds.max={fp_max:?}  tweak.eyeOffset={eye_off:?}"
    );
    eprintln!("=> eye_height = {:?}", player_eye_height(&ctx, &player));

    // ball camera
    match sm
      .get_member(&ctx, "cameraManager")
      .and_then(|c| c.get_member(&ctx, "ballCamera"))
    {
      Some(bc) => {
        eprintln!("ballCamera @ {:#010x}", bc.address);
        let btf = bc
          .get_member(&ctx, "transform")
          .and_then(|m| read_as_transform(&ctx, &m));
        eprintln!(
          "  ballCamera.transform translation = {:?}",
          btf.map(|m| m.w_axis.truncate())
        );
        let lp = bc
          .get_member(&ctx, "lookPos")
          .and_then(|m| read_as_vec3(&ctx, &m));
        eprintln!("  ballCamera.lookPos = {lp:?}");
      }
      None => eprintln!("ballCamera: <none>"),
    }

    // active camera per curCameraId (known-good path the renderer uses)
    if let Some(cm) = sm.get_member(&ctx, "cameraManager") {
      let cur = cm
        .get_member(&ctx, "curCameraId")
        .and_then(|m| m.read_u16(&ctx));
      eprintln!("curCameraId = {cur:?}");
    }
  }

  fn tri_at(z: f32, span: f32, mat: ECollisionMaterial) -> CollisionMesh {
    CollisionMesh {
      raw_verts: vec![
        Vec3::new(-span, -span, z),
        Vec3::new(span, -span, z),
        Vec3::new(0.0, span, z),
      ],
      raw_edges: vec![[0, 1], [1, 2], [2, 0]],
      raw_polys: vec![[0, 1, 2]],
      raw_poly_materials: vec![0],
      materials: vec![mat],
      ..Default::default()
    }
  }

  #[test]
  fn bezier_endpoints_and_midpoint() {
    let p = [
      Vec3::ZERO,
      Vec3::new(1.0, 0.0, 0.0),
      Vec3::new(2.0, 0.0, 0.0),
      Vec3::new(3.0, 0.0, 0.0),
    ];
    assert!((failsafe_spline_point(&p, 0.0) - p[0]).length() < 1e-6);
    assert!((failsafe_spline_point(&p, 1.0) - p[3]).length() < 1e-6);
    // colinear evenly-spaced control points -> straight line, param == position
    assert!((failsafe_spline_point(&p, 0.5) - Vec3::new(1.5, 0.0, 0.0)).length() < 1e-5);
  }

  #[test]
  fn ball_camera_filter_matches_the_include_exclude() {
    assert!(ball_camera_filter(ECollisionMaterial::SOLID));
    assert!(!ball_camera_filter(ECollisionMaterial(0))); // not Solid
    assert!(!ball_camera_filter(ECollisionMaterial(
      ECollisionMaterial::SOLID.0 | ECollisionMaterial::CAMERA_THRU.0
    )));
    assert!(!ball_camera_filter(ECollisionMaterial(
      ECollisionMaterial::SOLID.0 | ECollisionMaterial::SHOOT_THRU.0
    )));
  }

  #[test]
  fn clear_corridor_does_not_trigger() {
    // Camera behind the eye along -Y, nothing in the way.
    let inp = FailsafeInputs {
      cam_origin: Vec3::new(0.0, -5.0, 0.0),
      look_pos: Vec3::new(0.0, 0.0, 0.0),
      player_forward: Vec3::new(0.0, 1.0, 0.0),
      eye_pos: Vec3::new(0.0, 0.0, 0.0),
    };
    let meshes: Vec<CollisionMesh> = vec![tri_at(-50.0, 1.0, ECollisionMaterial::SOLID)];
    let p = predict_failsafe(&inp, meshes.iter());
    assert!(!p.would_trigger);
    assert_eq!(p.obstructed_segments, 0);
  }

  #[test]
  fn wall_across_the_spline_triggers() {
    // A big solid wall at y = -2.5 straddling the camera->eye spline.
    let inp = FailsafeInputs {
      cam_origin: Vec3::new(0.0, -5.0, 0.0),
      look_pos: Vec3::new(0.0, 0.0, 0.0),
      player_forward: Vec3::new(0.0, 1.0, 0.0),
      eye_pos: Vec3::new(0.0, 0.0, 0.0),
    };
    // wall in the X-Z plane at y = -2.5: rotate a big triangle there
    let wall = CollisionMesh {
      raw_verts: vec![
        Vec3::new(-100.0, -2.5, -100.0),
        Vec3::new(100.0, -2.5, -100.0),
        Vec3::new(0.0, -2.5, 100.0),
      ],
      raw_edges: vec![[0, 1], [1, 2], [2, 0]],
      raw_polys: vec![[0, 1, 2]],
      raw_poly_materials: vec![0],
      materials: vec![ECollisionMaterial::SOLID],
      ..Default::default()
    };
    let meshes = [wall];
    let p = predict_failsafe(&inp, meshes.iter());
    assert!(p.would_trigger);
    assert!(p.worst_separation > 0.3);
  }

  #[test]
  fn non_solid_wall_is_ignored() {
    let inp = FailsafeInputs {
      cam_origin: Vec3::new(0.0, -5.0, 0.0),
      look_pos: Vec3::new(0.0, 0.0, 0.0),
      player_forward: Vec3::new(0.0, 1.0, 0.0),
      eye_pos: Vec3::new(0.0, 0.0, 0.0),
    };
    let wall = CollisionMesh {
      raw_verts: vec![
        Vec3::new(-100.0, -2.5, -100.0),
        Vec3::new(100.0, -2.5, -100.0),
        Vec3::new(0.0, -2.5, 100.0),
      ],
      raw_edges: vec![[0, 1], [1, 2], [2, 0]],
      raw_polys: vec![[0, 1, 2]],
      raw_poly_materials: vec![0],
      // Solid but camera-through -> BallCameraFilter rejects it.
      materials: vec![ECollisionMaterial(
        ECollisionMaterial::SOLID.0 | ECollisionMaterial::CAMERA_THRU.0,
      )],
      ..Default::default()
    };
    let meshes = [wall];
    let p = predict_failsafe(&inp, meshes.iter());
    assert!(!p.would_trigger);
  }
}
