//! Predicts `CGameCollision::FindNonIntersectingVector`
//! (metaforce `Runtime/Collision/CGameCollision.cpp:933`) against the live
//! static collision world — the "shove the player out of geometry" reposition
//! failsafe.
//!
//! The game runs this from two sites, both of which hand
//! `FindNonIntersectingVector` a collision primitive and take the returned
//! offset as a straight `SetTranslation(pos + vec)`:
//! - `CPlayer::UpdatePlayer` while unmorphed (`CPlayer.cpp:1833`), the player's
//!   collision AABox expanded by `0.2` on every side. Gated by
//!   `CFailsafeTest::Passes()` (20 frames of near-stationary history).
//! - `CGameCollision::CollisionFailsafe` while morphed (`CGameCollision.cpp:915`),
//!   the raw morph-ball sphere. Gated by a stuck-tick counter.
//!
//! `FindNonIntersectingVector` itself sweeps 26 fixed directions (axes, edge and
//! corner diagonals) at growing radii (`i = 2, 3, 4, 6, 9 … < 1000`,
//! `pos = i * 0.005`, so ~0.01 → ~5.0 world units). A candidate offset `vec` is
//! accepted when all three hold:
//!
//! 1. `origOrigin + vec` is still inside the current area's AABB,
//! 2. the ray `center → center + vec` has a clear path
//!    (`CStateManager::RayCollideWorld`; `CAreaOctTree::LineTest` returns `false`
//!    on a hit, so "clear" == no hit),
//! 3. the primitive moved to `origOrigin + vec` no longer intersects static
//!    world geometry (`DetectCollisionBoolean_Cached`).
//!
//! The first accepted `vec` is returned; `None` if the whole sweep fails.
//!
//! Gate 2 is the interesting one: the game's octree `LineTest` **leaks through
//! geometry seams** — a segment grazing a leaf-node boundary can skip the
//! triangles stored in the node it barely misses. That is how a reposition ends
//! up flinging the player out of bounds: it accepts a `vec` whose straight-line
//! path visibly crosses a wall. Our [`raycast_world`] tests every triangle the
//! path could hit, so it never leaks. We keep both answers:
//! [`RepositionPrediction::selected_vec`] (all three gates, our strict ray test)
//! and [`RepositionPrediction::seam_leak_vec`] (gate 3 only — where the game's
//! leaky test lands when no clean escape exists).
//!
//! Whether the leak actually fires is a centimetre-scale floating-point matter
//! we can't reproduce from the triangle soup alone. Empirically (mem1 dumps) it
//! tracks whether the region has real open volume, and the game's own tell for
//! that is whether the larger `0.2`-expanded box finds a *clean*
//! `FindNonIntersectingVector` from the same pose — [`RepositionPrediction::region_open`].
//! No open box escape ⇒ enclosed ⇒ no leak ⇒ the failsafe just gives up
//! ([`RepositionOutcome::NoSafeSpot`]) instead of warping
//! ([`RepositionOutcome::SeamWarp`]).
//!
//! ## Deviations
//! - **Static world only.** No dynamic-actor collision / `nearList` — same scope
//!   cut as [`crate::world::ball_camera_failsafe`]. The ray gate collapses to a
//!   static [`raycast_world`] and the overlap test to the SAT tests below.
//! - **`CFailsafeTest::Passes()` is not ported.** Its pos/vel/input ring buffers
//!   aren't in the schema and can't be rebuilt from a snapshot. Instead
//!   [`RepositionPrediction::is_stuck`] reports whether the primitive intersects
//!   the world *right now* — the condition that makes the reposition move the
//!   player at all.
//! - **Hypothetical morph.** [`RepositionPrimSource::MorphBall`] runs the sweep
//!   against the morph-ball sphere even while unmorphed, to preview whether
//!   morphing here would leave the ball clipped and trip
//!   `CGameCollision::CollisionFailsafe` (`CGameCollision.cpp:886`, which always
//!   takes `CPlayer::GetCollisionPrimitive()` — the sphere once morphed).
//!   Morphing doesn't move the player's transform origin
//!   (`CPlayer::TransitionToMorphBallState`), so the sphere centre
//!   (`origin + (0,0,radius)`) is where the ball actually lands; the
//!   `CanEnterMorphBallState` gate that could block the morph outright is not
//!   modelled.
//! - **Area guard uses any loaded area's AABB** rather than
//!   `GetAreaAlways(GetNextAreaId())`, to skip an area-id lookup.
//! - Our AABox/sphere-vs-triangle SAT is not the game's octree traversal, the
//!   same way [`crate::world::ray_trace`] isn't; result parity is what matters
//!   for an overlay.

use glam::Vec3;

use crate::ctx::Ctx;
use crate::mem::globals::get_state_manager;
use crate::mem::math_utils::{read_as_transform, read_as_vec3};
use crate::structs::prime_structs::GameInstance;
use crate::world::bvh::Aabb;
use crate::world::collision_mesh::{CMaterialList, CollisionMesh};
use crate::world::ray_trace::{MaterialFilter, Ray, raycast_world};

/// `CActor`'s default material filter is `MakeIncludeExclude({Solid}, {0})`
/// (`Runtime/World/CActor.cpp:37`) and the player never overrides it, so a
/// triangle "collides" iff it carries the `Solid` bit.
pub fn player_collision_filter(m: CMaterialList) -> bool {
  m.contains(CMaterialList::SOLID)
}

// --- primitive-vs-triangle tests -------------------------------------------------

/// 13-axis SAT (Akenine–Möller) between an axis-aligned box (given by its centre
/// and positive half-extents) and a triangle. Touching counts as intersecting.
pub fn aabb_intersects_tri(box_center: Vec3, box_half: Vec3, tri: [Vec3; 3]) -> bool {
  // Work in box-local space (box centred at the origin).
  let v0 = tri[0] - box_center;
  let v1 = tri[1] - box_center;
  let v2 = tri[2] - box_center;
  let h = box_half;

  let f0 = v1 - v0;
  let f1 = v2 - v1;
  let f2 = v0 - v2;

  // 9 edge-cross axes: cross(box_axis, tri_edge).
  let axis_test = |a: Vec3| -> bool {
    if a.abs().max_element() < 1e-12 {
      return false; // degenerate axis (parallel edges) — not separating
    }
    let p0 = a.dot(v0);
    let p1 = a.dot(v1);
    let p2 = a.dot(v2);
    let r = h.x * a.x.abs() + h.y * a.y.abs() + h.z * a.z.abs();
    p0.min(p1).min(p2) > r || p0.max(p1).max(p2) < -r
  };

  for f in [f0, f1, f2] {
    if axis_test(Vec3::new(0.0, -f.z, f.y))
      || axis_test(Vec3::new(f.z, 0.0, -f.x))
      || axis_test(Vec3::new(-f.y, f.x, 0.0))
    {
      return false;
    }
  }

  // 3 box-face axes.
  let tmin = v0.min(v1).min(v2);
  let tmax = v0.max(v1).max(v2);
  if tmin.x > h.x || tmax.x < -h.x {
    return false;
  }
  if tmin.y > h.y || tmax.y < -h.y {
    return false;
  }
  if tmin.z > h.z || tmax.z < -h.z {
    return false;
  }

  // Triangle-plane axis.
  let n = f0.cross(f1);
  if n.abs().max_element() > 1e-12 {
    let s = n.dot(v0);
    let r = h.x * n.x.abs() + h.y * n.y.abs() + h.z * n.z.abs();
    if s.abs() > r {
      return false;
    }
  }

  true
}

/// Closest point on triangle `abc` to `p` (Ericson, *Real-Time Collision
/// Detection* §5.1.5).
fn closest_point_on_tri(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
  let ab = b - a;
  let ac = c - a;
  let ap = p - a;
  let d1 = ab.dot(ap);
  let d2 = ac.dot(ap);
  if d1 <= 0.0 && d2 <= 0.0 {
    return a;
  }
  let bp = p - b;
  let d3 = ab.dot(bp);
  let d4 = ac.dot(bp);
  if d3 >= 0.0 && d4 <= d3 {
    return b;
  }
  let vc = d1 * d4 - d3 * d2;
  if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
    let v = d1 / (d1 - d3);
    return a + ab * v;
  }
  let cp = p - c;
  let d5 = ab.dot(cp);
  let d6 = ac.dot(cp);
  if d6 >= 0.0 && d5 <= d6 {
    return c;
  }
  let vb = d5 * d2 - d1 * d6;
  if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
    let w = d2 / (d2 - d6);
    return a + ac * w;
  }
  let va = d3 * d6 - d5 * d4;
  if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
    let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
    return b + (c - b) * w;
  }
  let denom = 1.0 / (va + vb + vc);
  let v = vb * denom;
  let w = vc * denom;
  a + ab * v + ac * w
}

pub fn sphere_intersects_tri(center: Vec3, radius: f32, tri: [Vec3; 3]) -> bool {
  let q = closest_point_on_tri(center, tri[0], tri[1], tri[2]);
  (q - center).length_squared() <= radius * radius
}

// --- mesh / world overlap ------------------------------------------------------

fn mesh_overlap(
  mesh: &CollisionMesh,
  query: Aabb,
  filter: MaterialFilter,
  mut hit: impl FnMut([Vec3; 3]) -> bool,
) -> bool {
  let mut found = false;
  let mut test = |idx: usize| -> bool {
    let Some(tri) = mesh.master_list_triangle(idx) else {
      return false;
    };
    if !filter(tri.material) {
      return false;
    }
    if hit(tri.verts) {
      found = true;
      return true;
    }
    false
  };

  match mesh.bvh.as_ref().filter(|b| !b.is_empty()) {
    Some(bvh) => bvh.for_each_overlapping(query, |prim| test(prim as usize)),
    None => {
      for idx in 0..mesh.tri_count() {
        if test(idx) {
          break;
        }
      }
    }
  }
  found
}

fn world_intersects_aabb<'a>(
  meshes: impl IntoIterator<Item = &'a CollisionMesh>,
  min: Vec3,
  max: Vec3,
  filter: MaterialFilter,
) -> bool {
  let center = (min + max) * 0.5;
  let half = (max - min) * 0.5;
  let query = Aabb { min, max };
  meshes
    .into_iter()
    .any(|m| mesh_overlap(m, query, filter, |t| aabb_intersects_tri(center, half, t)))
}

fn world_intersects_sphere<'a>(
  meshes: impl IntoIterator<Item = &'a CollisionMesh>,
  center: Vec3,
  radius: f32,
  filter: MaterialFilter,
) -> bool {
  let query = Aabb {
    min: center - radius,
    max: center + radius,
  };
  meshes.into_iter().any(|m| {
    mesh_overlap(m, query, filter, |t| {
      sphere_intersects_tri(center, radius, t)
    })
  })
}

// --- prediction ---------------------------------------------------------------

/// The player collision primitive, already resolved to world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RepositionPrim {
  /// `0.2`-expanded when it comes from the unmorphed player path.
  Aabox {
    world_min: Vec3,
    world_max: Vec3,
  },
  Sphere {
    world_center: Vec3,
    radius: f32,
  },
}

impl RepositionPrim {
  /// This primitive translated by `off`.
  fn translated(self, off: Vec3) -> RepositionPrim {
    match self {
      RepositionPrim::Aabox {
        world_min,
        world_max,
      } => RepositionPrim::Aabox {
        world_min: world_min + off,
        world_max: world_max + off,
      },
      RepositionPrim::Sphere {
        world_center,
        radius,
      } => RepositionPrim::Sphere {
        world_center: world_center + off,
        radius,
      },
    }
  }

  fn intersects_world<'a>(
    self,
    meshes: impl IntoIterator<Item = &'a CollisionMesh> + Clone,
    filter: MaterialFilter,
  ) -> bool {
    match self {
      RepositionPrim::Aabox {
        world_min,
        world_max,
      } => world_intersects_aabb(meshes, world_min, world_max, filter),
      RepositionPrim::Sphere {
        world_center,
        radius,
      } => world_intersects_sphere(meshes, world_center, radius, filter),
    }
  }

  /// `prim.CalculateAABox(xf).center()`.
  fn center(self) -> Vec3 {
    match self {
      RepositionPrim::Aabox {
        world_min,
        world_max,
      } => (world_min + world_max) * 0.5,
      RepositionPrim::Sphere { world_center, .. } => world_center,
    }
  }
}

/// Which collision primitive [`predict_reposition_from_live`] evaluates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RepositionPrimSource {
  /// The player's current primitive: the `0.2`-expanded AABox unmorphed, the
  /// morph-ball sphere morphed. What the game's failsafe actually runs.
  Live,
  /// Force the morph-ball sphere regardless of morph state — "if I morphed right
  /// here, would the ball be clipped into terrain?". Identical to [`Self::Live`]
  /// once the player is already morphed.
  MorphBall,
  /// Force the `0.2`-expanded unmorphed collision AABox regardless of morph
  /// state (diagnostic use).
  #[allow(dead_code)]
  UnmorphedBox,
}

#[derive(Clone, Copy, Debug)]
pub struct RepositionInputs {
  pub prim: RepositionPrim,
  /// `xf.origin` — player translation plus `primitiveOffset` (≈ translation).
  pub orig_origin: Vec3,
  /// Player translation; `resulting_pos = player_pos + selected_vec`.
  pub player_pos: Vec3,
}

/// One evaluated candidate from the 26-direction × growing-radius sweep.
/// `vec` is carried for inspection/tests; the overlay draws from [`Self::end_point`].
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub struct RepositionAttempt {
  pub vec: Vec3,
  /// `center + vec` — the ray-gate endpoint (also where the overlay draws to).
  pub end_point: Vec3,
  pub in_area: bool,
  /// Path `center → center + vec` is unobstructed (only evaluated when `in_area`).
  pub ray_clear: bool,
  /// Primitive at `orig_origin + vec` no longer intersects the world (only
  /// evaluated when `in_area`).
  pub prim_clear: bool,
  pub selected: bool,
}

/// What `CGameCollision::CollisionFailsafe` would do with this primitive once
/// its stuck-tick gate elapses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RepositionOutcome {
  /// Not clipped — the failsafe leaves the player alone.
  Clear,
  /// Clipped, and the sweep found an offset that clears both our ray gate and
  /// the overlap test. The player is moved to `resulting_pos`.
  Nudged,
  /// Clipped; no offset clears our (non-leaky) ray gate, but one clears the
  /// overlap test *and* a larger probe primitive finds real open space here
  /// (`region_open`). The game's octree ray test leaks through the thin geometry
  /// in between and returns that offset anyway — warping the player to
  /// [`RepositionPrediction::destination_center`], almost always out of bounds.
  /// This is the failure this overlay is here to catch.
  SeamWarp,
  /// Clipped, and either nothing in range clears the overlap test, or the region
  /// around the primitive is enclosed (`!region_open`) so the octree ray test
  /// won't leak. The failsafe gives up (halves the stored velocity, leaves the
  /// player embedded); collision resolution then squeezes the primitive out over
  /// many frames, or it just stays stuck.
  NoSafeSpot,
}

#[derive(Clone, Debug)]
pub struct RepositionPrediction {
  /// The collision primitive the sweep ran against, in world space.
  pub prim: RepositionPrim,
  /// The primitive intersects static geometry at the current pose — the game's
  /// reposition would actually move the player.
  pub is_stuck: bool,
  /// `prim.CalculateAABox(xf).center()` — the origin every candidate ray fans
  /// out from.
  pub center: Vec3,
  pub attempts: Vec<RepositionAttempt>,
  /// First swept offset that clears all three gates (in-area, ray, overlap).
  pub selected_vec: Option<Vec3>,
  /// First swept offset that clears the in-area + overlap gates but *not* our
  /// ray gate — where the game's leaky octree ray test warps the player when no
  /// clean escape exists. `Some` only when `selected_vec` is `None` *and*
  /// `region_open`.
  pub seam_leak_vec: Option<Vec3>,
  /// A larger probe primitive (the `0.2`-expanded unmorphed box) finds a fully
  /// clean escape from this pose — evidence the surrounding geometry is sparse
  /// enough that the game's octree ray test will leak. Gates [`Self::seam_leak_vec`].
  pub region_open: bool,
  /// `player_pos + selected_vec`.
  pub resulting_pos: Option<Vec3>,
}

impl RepositionPrediction {
  /// Record whether a larger probe primitive found real open space from this
  /// pose. When `false`, the seam-leak prediction is retracted (the game's
  /// octree ray test won't leak through enclosed geometry).
  pub fn set_region_open(&mut self, open: bool) {
    self.region_open = open;
    if !open {
      self.seam_leak_vec = None;
    }
  }

  pub fn outcome(&self) -> RepositionOutcome {
    match (self.is_stuck, self.selected_vec, self.seam_leak_vec) {
      (false, _, _) => RepositionOutcome::Clear,
      (true, Some(_), _) => RepositionOutcome::Nudged,
      (true, None, Some(_)) => RepositionOutcome::SeamWarp,
      (true, None, None) => RepositionOutcome::NoSafeSpot,
    }
  }

  /// The primitive's post-reposition centre — the destination the overlay draws
  /// the ghost primitive at. `None` for [`RepositionOutcome::Clear`] /
  /// [`RepositionOutcome::NoSafeSpot`].
  pub fn destination_center(&self) -> Option<Vec3> {
    let off = self.selected_vec.or(self.seam_leak_vec)?;
    Some(self.center + off)
  }
}

/// `FindNonIntersectingVector`'s radius schedule: `for (i = 2; i < 1000; i += i/2)`,
/// scaled by `0.005`.
fn radius_steps() -> impl Iterator<Item = f32> {
  std::iter::successors(Some(2i32), |&i| {
    let n = i + i / 2;
    (n < 1000).then_some(n)
  })
  .map(|i| i as f32 * 0.005)
}

/// The 26 `switch (j)` directions (`CGameCollision.cpp:944-1025`) as sign
/// vectors; multiplied by the current radius. Order matters — the game returns
/// the first hit.
#[rustfmt::skip]
const DIRS: [[f32; 3]; 26] = [
  [ 0.0,  1.0,  0.0], [ 0.0, -1.0,  0.0], [ 1.0,  0.0,  0.0], [-1.0,  0.0,  0.0],
  [ 0.0,  0.0,  1.0], [ 0.0,  0.0, -1.0], [ 0.0,  1.0,  1.0], [ 0.0, -1.0, -1.0],
  [ 0.0, -1.0,  1.0], [ 0.0,  1.0, -1.0], [ 1.0,  0.0,  1.0], [-1.0,  0.0, -1.0],
  [-1.0,  0.0,  1.0], [ 1.0,  0.0, -1.0], [ 1.0,  1.0,  0.0], [-1.0, -1.0,  0.0],
  [-1.0,  1.0,  0.0], [ 1.0, -1.0,  0.0], [ 1.0,  1.0,  1.0], [-1.0,  1.0,  1.0],
  [ 1.0, -1.0,  1.0], [-1.0, -1.0,  1.0], [ 1.0,  1.0, -1.0], [-1.0,  1.0, -1.0],
  [ 1.0, -1.0, -1.0], [-1.0, -1.0, -1.0],
];

/// Insurance bound on candidates recorded — 26 dirs × 16 radius steps = 416 in
/// the worst (fully stuck) case, so this is never hit in practice.
const MAX_ATTEMPTS: usize = 4096;

/// Pure core: port of `FindNonIntersectingVector`. `meshes` is iterated once per
/// candidate, so it must be cheaply `Clone` (e.g. `HashMap::values()`).
///
/// The returned prediction assumes `region_open == true` (seam-leak surfaced
/// whenever a ray-blocked overlap-clear candidate exists). Callers with a
/// larger probe primitive should set [`RepositionPrediction::region_open`]
/// afterwards via [`RepositionPrediction::set_region_open`].
pub fn predict_reposition<'a>(
  inp: &RepositionInputs,
  meshes: impl IntoIterator<Item = &'a CollisionMesh> + Clone,
  area_aabbs: &[Aabb],
) -> RepositionPrediction {
  let filter: MaterialFilter = &player_collision_filter;
  let center = inp.prim.center();

  let is_stuck = inp.prim.intersects_world(meshes.clone(), filter);

  let mut attempts: Vec<RepositionAttempt> = Vec::new();
  let mut selected_vec = None;
  // First in-area candidate whose overlap test clears — regardless of the ray
  // gate. The game's octree `LineTest` leaks through thin geometry seams, so
  // when our stricter raycast blocks every clean escape the game still returns
  // this one and warps the player (usually out of bounds). Superset of
  // `selected_vec`, so it is always set by the time we break on a strict hit.
  let mut prim_clear_vec = None;

  'sweep: for radius in radius_steps() {
    for dir in DIRS {
      let vec = Vec3::from_array(dir) * radius;
      let world_point = inp.orig_origin + vec;
      let end_point = center + vec;

      let in_area = area_aabbs.iter().any(|a| a.contains_point(world_point));
      let mut ray_clear = false;
      let mut prim_clear = false;

      if in_area {
        let mag = vec.length();
        // RayCollideWorld: a degenerate delta is "clear"; otherwise clear ==
        // the static ray finds no Solid triangle within `mag`.
        ray_clear = mag <= 1e-6
          || raycast_world(
            meshes.clone(),
            Ray {
              origin: center,
              dir: vec / mag,
            },
            mag,
            filter,
          )
          .is_none();

        prim_clear = !inp
          .prim
          .translated(vec)
          .intersects_world(meshes.clone(), filter);
      }

      let selected = in_area && ray_clear && prim_clear;
      attempts.push(RepositionAttempt {
        vec,
        end_point,
        in_area,
        ray_clear,
        prim_clear,
        selected,
      });

      if in_area && prim_clear && prim_clear_vec.is_none() {
        prim_clear_vec = Some(vec);
      }
      if selected {
        selected_vec = Some(vec);
        break 'sweep;
      }
      if attempts.len() >= MAX_ATTEMPTS {
        break 'sweep;
      }
    }
  }

  // Surface the seam-leak target when no strictly-clean escape was found;
  // `set_region_open(false)` clears it later if the region turns out enclosed.
  let seam_leak_vec = if selected_vec.is_none() {
    prim_clear_vec
  } else {
    None
  };

  RepositionPrediction {
    prim: inp.prim,
    is_stuck,
    center,
    attempts,
    selected_vec,
    seam_leak_vec,
    region_open: true,
    resulting_pos: selected_vec.map(|v| inp.player_pos + v),
  }
}

// --- live reader --------------------------------------------------------------

fn read_vec3_member(ctx: &Ctx, parent: &GameInstance, name: &str) -> Option<Vec3> {
  read_as_vec3(ctx, &parent.get_member(ctx, name)?)
}

/// `EPlayerMorphBallState::Morphed`.
const MORPH_STATE_MORPHED: u32 = 1;
/// `CPlayer.cpp:1836` expands the collision box by this on every side before the
/// sweep.
const AABOX_EXPAND: f32 = 0.2;

/// Read the live player primitive + pose and run [`predict_reposition`].
///
/// [`RepositionPrimSource::Live`]: unmorphed → the `0.2`-expanded collision
/// AABox, morphed → the raw morph-ball sphere.
/// [`RepositionPrimSource::MorphBall`]: always the sphere, for a "what if I
/// morphed here" preview. Returns `None` if any required read fails.
pub fn predict_reposition_from_live<'a>(
  ctx: &Ctx,
  meshes: impl IntoIterator<Item = &'a CollisionMesh> + Clone,
  area_aabbs: &[Aabb],
  prim_source: RepositionPrimSource,
) -> Option<RepositionPrediction> {
  let player = get_state_manager().get_member(ctx, "player")?;

  let player_pos = read_as_transform(ctx, &player.get_member(ctx, "transform")?)?
    .w_axis
    .truncate();
  let prim_offset = read_vec3_member(ctx, &player, "primitiveOffset").unwrap_or(Vec3::ZERO);
  let orig_origin = player_pos + prim_offset;

  let morphed = player
    .get_member(ctx, "morphState")
    .and_then(|m| m.read_u32(ctx))
    .map(|s| s == MORPH_STATE_MORPHED)
    .unwrap_or(false);

  let use_sphere = match prim_source {
    RepositionPrimSource::Live => morphed,
    RepositionPrimSource::MorphBall => true,
    RepositionPrimSource::UnmorphedBox => false,
  };

  // The `0.2`-expanded unmorphed AABox — the game's failsafe primitive while
  // unmorphed, and also our "is the region open here" probe.
  let box_prim = {
    let aabb = player
      .get_member(ctx, "collisionPrimitive")?
      .get_member(ctx, "aabb")?;
    let local_min = read_vec3_member(ctx, &aabb, "min")?;
    let local_max = read_vec3_member(ctx, &aabb, "max")?;
    RepositionPrim::Aabox {
      world_min: orig_origin + local_min - AABOX_EXPAND,
      world_max: orig_origin + local_max + AABOX_EXPAND,
    }
  };

  let prim = if use_sphere {
    let sphere = player
      .get_member(ctx, "morphBall")?
      .get_member(ctx, "collisionSphere")?
      .get_member(ctx, "sphere")?;
    let local_center = read_vec3_member(ctx, &sphere, "origin")?;
    let radius = sphere.get_member(ctx, "radius")?.read_f32(ctx)?;
    RepositionPrim::Sphere {
      world_center: orig_origin + local_center,
      radius,
    }
  } else {
    box_prim
  };

  let inputs = |prim| RepositionInputs {
    prim,
    orig_origin,
    player_pos,
  };

  let mut pred = predict_reposition(&inputs(prim), meshes.clone(), area_aabbs);

  // A seam-warp only happens where the geometry is sparse enough for the game's
  // octree ray test to leak. Proxy: the larger `0.2`-box primitive finds a
  // strictly-clean escape from the same pose. Only worth probing when we
  // actually have a seam-leak candidate to gate.
  if pred.seam_leak_vec.is_some() {
    let region_open = if prim == box_prim {
      false // the box already failed to find a clean escape (it *is* this prediction)
    } else {
      predict_reposition(&inputs(box_prim), meshes, area_aabbs)
        .selected_vec
        .is_some()
    };
    pred.set_region_open(region_open);
  }
  Some(pred)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn tri(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [Vec3; 3] {
    [
      Vec3::from_array(a),
      Vec3::from_array(b),
      Vec3::from_array(c),
    ]
  }

  /// A large solid slab (two triangles) lying in the plane `z = h`.
  fn slab_at_z(h: f32) -> CollisionMesh {
    CollisionMesh {
      raw_verts: vec![
        Vec3::new(-50.0, -50.0, h),
        Vec3::new(50.0, -50.0, h),
        Vec3::new(50.0, 50.0, h),
        Vec3::new(-50.0, 50.0, h),
      ],
      raw_edges: vec![[0, 1], [1, 2], [2, 0], [2, 3], [3, 0]],
      raw_polys: vec![[0, 1, 2], [3, 4, 2]],
      raw_poly_materials: vec![0, 0],
      materials: vec![CMaterialList::SOLID],
      ..Default::default()
    }
  }

  /// A big solid wall in the plane `x = k`.
  fn wall_at_x(k: f32) -> CollisionMesh {
    CollisionMesh {
      raw_verts: vec![
        Vec3::new(k, -50.0, -50.0),
        Vec3::new(k, 50.0, -50.0),
        Vec3::new(k, 50.0, 50.0),
        Vec3::new(k, -50.0, 50.0),
      ],
      raw_edges: vec![[0, 1], [1, 2], [2, 0], [2, 3], [3, 0]],
      raw_polys: vec![[0, 1, 2], [3, 4, 2]],
      raw_poly_materials: vec![0, 0],
      materials: vec![CMaterialList::SOLID],
      ..Default::default()
    }
  }

  fn big_area() -> [Aabb; 1] {
    [Aabb {
      min: Vec3::splat(-100.0),
      max: Vec3::splat(100.0),
    }]
  }

  #[test]
  fn aabb_tri_basic_hit_and_miss() {
    let t = tri([-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]);
    assert!(aabb_intersects_tri(Vec3::ZERO, Vec3::splat(0.5), t));
    // box well above the triangle plane
    assert!(!aabb_intersects_tri(
      Vec3::new(0.0, 0.0, 5.0),
      Vec3::splat(0.5),
      t
    ));
    // box beside the triangle in x
    assert!(!aabb_intersects_tri(
      Vec3::new(10.0, 0.0, 0.0),
      Vec3::splat(0.5),
      t
    ));
  }

  #[test]
  fn aabb_tri_edge_cross_axis_separates_diagonal() {
    // A thin box near a 45° edge that no face axis separates but an edge-cross
    // axis does.
    let t = tri([0.0, 0.0, 0.0], [4.0, 4.0, 0.0], [4.0, 0.0, 0.0]);
    assert!(!aabb_intersects_tri(
      Vec3::new(0.5, 2.0, 0.0),
      Vec3::splat(0.3),
      t
    ));
    assert!(aabb_intersects_tri(
      Vec3::new(2.0, 1.0, 0.0),
      Vec3::splat(0.3),
      t
    ));
  }

  #[test]
  fn sphere_tri_hit_miss_and_edge() {
    let t = tri([-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]);
    assert!(sphere_intersects_tri(Vec3::new(0.0, 0.0, 0.4), 0.5, t));
    assert!(!sphere_intersects_tri(Vec3::new(0.0, 0.0, 0.4), 0.3, t));
    // closest feature is the vertex (1,-1,0)
    assert!(sphere_intersects_tri(Vec3::new(1.4, -1.4, 0.0), 0.6, t));
    assert!(!sphere_intersects_tri(Vec3::new(1.4, -1.4, 0.0), 0.5, t));
  }

  #[test]
  fn player_collision_filter_wants_solid() {
    assert!(player_collision_filter(CMaterialList::SOLID));
    assert!(!player_collision_filter(CMaterialList::FLOOR));
    assert!(player_collision_filter(CMaterialList(
      CMaterialList::SOLID.0 | CMaterialList::FLOOR.0
    )));
  }

  #[test]
  fn radius_schedule_matches_the_cpp_loop() {
    let got: Vec<i32> = radius_steps().map(|r| (r / 0.005).round() as i32).collect();
    assert_eq!(
      got,
      vec![
        2, 3, 4, 6, 9, 13, 19, 28, 42, 63, 94, 141, 211, 316, 474, 711
      ]
    );
  }

  #[test]
  fn clear_box_is_not_stuck_and_barely_moves() {
    // Player box floating well above a floor.
    let inp = RepositionInputs {
      prim: RepositionPrim::Aabox {
        world_min: Vec3::new(-0.5, -0.5, 10.0),
        world_max: Vec3::new(0.5, 0.5, 12.7),
      },
      orig_origin: Vec3::new(0.0, 0.0, 10.0),
      player_pos: Vec3::new(0.0, 0.0, 10.0),
    };
    let meshes = [slab_at_z(0.0)];
    let p = predict_reposition(&inp, meshes.iter(), &big_area());
    assert!(!p.is_stuck);
    let v = p
      .selected_vec
      .expect("a near-zero offset is always accepted");
    assert!(v.length() < 0.05, "expected sub-cm offset, got {v:?}");
    // first direction tried is +Y at the smallest radius
    assert_eq!(p.attempts.first().unwrap().vec, Vec3::new(0.0, 0.01, 0.0));
  }

  /// A player box clipping a wall at `x = 0` by 5cm — its centre is on the open
  /// (`+x`) side, so a small `+x` nudge frees it and the escape ray never
  /// re-enters the wall. This is the realistic "stuck" shape (slight overlap,
  /// centre outside the solid), unlike a box perfectly centred on a surface,
  /// which this algorithm — in the game too — cannot ray-gate its way out of.
  fn box_clipping_wall_x0() -> RepositionInputs {
    RepositionInputs {
      prim: RepositionPrim::Aabox {
        world_min: Vec3::new(-0.05, -0.5, -0.5),
        world_max: Vec3::new(0.35, 0.5, 0.5),
      },
      orig_origin: Vec3::new(0.15, 0.0, 0.0),
      player_pos: Vec3::new(0.15, 0.0, 0.0),
    }
  }

  #[test]
  fn embedded_box_is_stuck_and_gets_pushed_clear() {
    let inp = box_clipping_wall_x0();
    let meshes = [wall_at_x(0.0)];
    let p = predict_reposition(&inp, meshes.iter(), &big_area());
    assert!(p.is_stuck);
    let v = p.selected_vec.expect("a small +x nudge frees the box");
    assert!(v.x > 0.0, "got {v:?}");
    assert!(!world_intersects_aabb(
      meshes.iter(),
      Vec3::new(-0.05, -0.5, -0.5) + v,
      Vec3::new(0.35, 0.5, 0.5) + v,
      &player_collision_filter,
    ));
    assert_eq!(p.resulting_pos, Some(inp.player_pos + v));
  }

  #[test]
  fn ray_gate_rejects_a_direction_walled_off_from_open_space() {
    let inp = box_clipping_wall_x0();

    // Without a blocker, the sweep escapes straight out along +x.
    let open = [wall_at_x(0.0)];
    let free = predict_reposition(&inp, open.iter(), &big_area());
    let v = free.selected_vec.expect("escapes +x");
    assert!(v.x > 0.0 && v.y == 0.0 && v.z == 0.0, "got {v:?}");

    // Box it in: a second wall just past the box on the +x side. Now the only
    // direction that would free the primitive has its escape ray blocked, so the
    // sweep finds nothing.
    let boxed_in = [wall_at_x(0.0), wall_at_x(0.21)];
    let p = predict_reposition(&inp, boxed_in.iter(), &big_area());
    assert!(p.is_stuck);
    assert!(
      p.selected_vec.is_none(),
      "walled in on both sides, no escape exists"
    );
    // The gate actually fired: some +x candidate that clears the geometry was
    // rejected purely because its ray crossed the x=0.21 wall.
    assert!(
      p.attempts.iter().any(|a| a.vec.x > 0.0
        && a.vec.y == 0.0
        && a.vec.z == 0.0
        && a.in_area
        && !a.ray_clear),
      "expected at least one +x attempt rejected by the ray gate"
    );
  }

  /// The primitive can only reach open space by passing straight through the
  /// wall it is embedded in (every clean-ray direction is walled off or leaves
  /// the area). The game's leaky octree ray test accepts that anyway and warps
  /// the player out of bounds — [`RepositionOutcome::SeamWarp`].
  #[test]
  fn only_escape_is_through_the_wall_seam() {
    let inp = RepositionInputs {
      prim: RepositionPrim::Aabox {
        world_min: Vec3::new(-0.35, -0.3, -0.3),
        world_max: Vec3::new(0.25, 0.3, 0.3),
      },
      orig_origin: Vec3::ZERO,
      player_pos: Vec3::ZERO,
    };
    let meshes = [wall_at_x(0.0)];
    // Area cut off just behind the box: backing out (-x) leaves the area, and
    // no lateral move clears the x=0 plane, so the only overlap-free offset is
    // forward through the wall — where the escape ray is blocked.
    let area = [Aabb {
      min: Vec3::new(-0.2, -50.0, -50.0),
      max: Vec3::new(50.0, 50.0, 50.0),
    }];
    let mut p = predict_reposition(&inp, meshes.iter(), &area);
    assert!(p.is_stuck);
    assert_eq!(p.selected_vec, None, "every clean-ray escape is blocked");
    let v = p.seam_leak_vec.expect("forward-through-the-wall offset");
    assert!(v.x > 0.0, "seam leak points through the wall, got {v:?}");
    assert_eq!(p.outcome(), RepositionOutcome::SeamWarp);
    assert_eq!(p.destination_center(), Some(inp.prim.center() + v));

    // If a larger probe primitive also can't find open space here, the octree
    // ray test won't leak — the seam warp is retracted.
    p.set_region_open(false);
    assert_eq!(p.seam_leak_vec, None);
    assert_eq!(p.destination_center(), None);
    assert_eq!(p.outcome(), RepositionOutcome::NoSafeSpot);
  }

  #[test]
  fn out_of_area_candidates_are_never_selected() {
    let inp = RepositionInputs {
      prim: RepositionPrim::Aabox {
        world_min: Vec3::new(-0.5, -0.5, -0.2),
        world_max: Vec3::new(0.5, 0.5, 0.2),
      },
      orig_origin: Vec3::ZERO,
      player_pos: Vec3::ZERO,
    };
    let meshes = [slab_at_z(0.0)];
    // A pinhole area AABB around the origin: any real displacement leaves it.
    let tiny = [Aabb {
      min: Vec3::splat(-0.001),
      max: Vec3::splat(0.001),
    }];
    let p = predict_reposition(&inp, meshes.iter(), &tiny);
    assert!(p.selected_vec.is_none());
    assert!(
      p.attempts
        .iter()
        .all(|a| !a.in_area || a.vec.length() < 0.01)
    );
  }

  #[test]
  #[ignore = "diagnostic dump; run explicitly with --nocapture"]
  fn dump_reposition_inputs_from_live() {
    use crate::mem::game_memory::GameMemory;
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

    let player = get_state_manager()
      .get_member(&ctx, "player")
      .expect("player");
    eprintln!("player @ {:#010x}", player.address);
    let tf = read_as_transform(&ctx, &player.get_member(&ctx, "transform").unwrap()).unwrap();
    eprintln!("player_pos = {:?}", tf.w_axis.truncate());
    eprintln!(
      "primitiveOffset = {:?}",
      read_vec3_member(&ctx, &player, "primitiveOffset")
    );
    eprintln!(
      "morphState = {:?}",
      player
        .get_member(&ctx, "morphState")
        .and_then(|m| m.read_u32(&ctx))
    );
    let aabb = player
      .get_member(&ctx, "collisionPrimitive")
      .and_then(|m| m.get_member(&ctx, "aabb"));
    eprintln!(
      "collisionPrimitive.aabb = {:?} .. {:?}",
      aabb.as_ref().and_then(|a| read_vec3_member(&ctx, a, "min")),
      aabb.as_ref().and_then(|a| read_vec3_member(&ctx, a, "max")),
    );
    if let Some(s) = player
      .get_member(&ctx, "morphBall")
      .and_then(|m| m.get_member(&ctx, "collisionSphere"))
      .and_then(|m| m.get_member(&ctx, "sphere"))
    {
      eprintln!(
        "morphBall sphere: origin={:?} radius={:?}",
        read_vec3_member(&ctx, &s, "origin"),
        s.get_member(&ctx, "radius").and_then(|m| m.read_f32(&ctx)),
      );
    }
  }

  #[test]
  #[ignore = "diagnostic dump; run explicitly with --nocapture"]
  fn dump_reposition_prediction_from_live() {
    use crate::mem::area_utils::get_areas;
    use crate::mem::game_memory::GameMemory;
    use crate::structs::prime_structs::GameStructs;
    use crate::world::collision_mesh::load_mesh;

    let path = std::env::var("PRIMEWATCH_MEM1_RAW")
      .unwrap_or_else(|_| format!("{}/mem1.raw", env!("CARGO_MANIFEST_DIR")));
    if !std::path::Path::new(&path).exists() {
      eprintln!("skipping: {path} not found");
      return;
    }
    let mut mem = GameMemory::new();
    mem.load_from_file(&path).expect("read dump");
    let mut structs = GameStructs::new_empty();
    structs
      .load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/prime_defs"))
      .expect("load prime_defs");
    let ctx = Ctx::new(&structs, &mem);

    let meshes: Vec<CollisionMesh> = get_areas(&ctx)
      .iter()
      .filter_map(|a| load_mesh(&ctx, a))
      .collect();
    let area_aabbs: Vec<Aabb> = meshes
      .iter()
      .map(|m| Aabb {
        min: m.min,
        max: m.max,
      })
      .collect();

    let player = get_state_manager().get_member(&ctx, "player").unwrap();
    let player_pos = read_as_transform(&ctx, &player.get_member(&ctx, "transform").unwrap())
      .unwrap()
      .w_axis
      .truncate();
    eprintln!(
      "{path}: {} meshes, morphState={:?}",
      meshes.len(),
      player
        .get_member(&ctx, "morphState")
        .and_then(|m| m.read_u32(&ctx)),
    );
    eprintln!(
      "  player_pos={player_pos:?} lastNonColliding={:?} velocity={:?} numTicksStuck={:?}",
      player
        .get_member(&ctx, "lastNonCollidingState")
        .and_then(|m| read_vec3_member(&ctx, &m, "translation")),
      read_vec3_member(&ctx, &player, "velocity"),
      ctx.mem.read_u32(player.address + 0x24c),
    );

    for src in [
      RepositionPrimSource::UnmorphedBox,
      RepositionPrimSource::MorphBall,
    ] {
      match predict_reposition_from_live(&ctx, meshes.iter(), &area_aabbs, src) {
        None => eprintln!("{src:?}: prediction returned None (a read failed)"),
        Some(p) => eprintln!(
          "{src:?}: outcome={:?} selected_vec={:?} seam_leak_vec={:?}",
          p.outcome(),
          p.selected_vec,
          p.seam_leak_vec,
        ),
      }
    }
  }
}
