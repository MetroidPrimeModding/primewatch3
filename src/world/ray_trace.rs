//! Static world ray cast — the static half of `CGameCollision::RayWorldIntersection`.
//!
//! See `src/world/collision_ray_tracing.md` for the full research notes. This is
//! the "recommended implementation" from §7: for each area's already-parsed
//! [`CollisionMesh`], reconstruct master-list triangles (§5.2), run the
//! two-sided Möller–Trumbore test the octree uses (§5.1), and keep the nearest
//! hit. Not the game's octree — candidate triangles come from the mesh's own
//! [`Bvh`](crate::world::bvh::Bvh) (a plain median-split tree built at load
//! time; brute-force fallback when it is absent). A BVH descent and a min-`t`
//! scan return the same triangle for any ray that isn't grazing a node boundary
//! at almost exactly the hit distance, which does not matter for an inspection
//! overlay.
//!
//! The material filter (§9) is a caller-supplied [`MaterialFilter`] predicate
//! (`|_| true` / [`pass_everything`] is the `skPassEverything` default). A full
//! `CMaterialFilter` include/exclude port can wrap into that predicate; see
//! `ball_camera_failsafe::ball_camera_filter` for the first real one.

use glam::Vec3;

use crate::world::collision_mesh::{CMaterialList, CollisionMesh};

/// A world-space ray. `dir` is expected to be unit length, so `t` values are in
/// world units.
#[derive(Clone, Copy, Debug)]
pub struct Ray {
  pub origin: Vec3,
  pub dir: Vec3,
}

/// A single triangle hit.
#[derive(Clone, Copy, Debug)]
pub struct RayHit {
  /// Distance along `dir` (world units).
  pub t: f32,
  /// `origin + dir * t`.
  pub point: Vec3,
  /// Surface normal, `normalize((v1 - v0) × (v2 - v0))` with the winding from
  /// [`CollisionMesh::master_list_triangle`] (doc §5.3).
  pub normal: Vec3,
  /// Index into the master triangle list (`0..polyCount`).
  pub tri_index: usize,
  /// The triangle's 32-bit surface material word.
  pub material: CMaterialList,
}

/// Two-sided Möller–Trumbore, inlined exactly as the octree leaf test does it
/// (doc §5.1 / `CAreaOctTree.cpp`). Two-sided: a negative determinant (back
/// face) still hits.
///
/// Returns the ray parameter `t` when the ray strikes triangle `(v0, v1, v2)`
/// within `[lo, hi)`, else `None`. The reject order (compute and test `t`
/// before `v`) matches the C++.
pub fn moller_trumbore_two_sided(
  origin: Vec3,
  dir: Vec3,
  v0: Vec3,
  v1: Vec3,
  v2: Vec3,
  lo: f32,
  hi: f32,
) -> Option<f32> {
  // C++ uses FLT_EPSILON for the determinant reject, scaled by 10.
  const DET_EPS: f32 = f32::EPSILON * 10.0;

  let e0 = v1 - v0;
  let e1 = v2 - v0;
  let p = dir.cross(e1);
  let det = p.dot(e0);
  if det.abs() < DET_EPS {
    return None; // ray parallel to the triangle plane
  }
  let inv_det = 1.0 / det;

  let tvec = origin - v0;
  let u = inv_det * tvec.dot(p);
  if !(0.0..=1.0).contains(&u) {
    return None;
  }

  let q = tvec.cross(e0);
  let t = inv_det * q.dot(e1);
  if t >= hi || t < lo {
    return None;
  }

  let v = inv_det * q.dot(dir);
  if v < 0.0 || u + v > 1.0 {
    return None;
  }

  Some(t)
}

/// Triangle-facing filter for a cast, mirroring the renderer's Culling menu.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TriCull {
  #[default]
  None,
  FrontOnly,
  BackOnly,
}

impl TriCull {
  /// Whether a triangle with outward `normal` survives this cull for a ray
  /// traveling `dir`. A `None` normal (render soup not built) always passes.
  fn keeps(self, normal: Option<Vec3>, dir: Vec3) -> bool {
    let Some(n) = normal else {
      return true;
    };
    match self {
      TriCull::None => true,
      TriCull::FrontOnly => n.dot(dir) < 0.0,
      TriCull::BackOnly => n.dot(dir) > 0.0,
    }
  }
}

pub type MaterialFilter<'a> = &'a dyn Fn(CMaterialList) -> bool;

pub fn pass_everything(_: CMaterialList) -> bool {
  true
}

/// Cast against the master triangle list of one area mesh (doc §7).
///
/// Uses the mesh's [`Bvh`](crate::world::bvh::Bvh) when
/// [`build_bvh`](CollisionMesh::build_bvh) has run (every `load_mesh` result);
/// otherwise brute-forces every triangle. Both paths return the identical
/// nearest hit — the BVH only changes which triangles get the Möller–Trumbore
/// test, not the result.
///
/// `max_t` bounds the search (world units); pass `<= 0.0` for an unbounded ray.
/// `filter` is the `CMaterialFilter` hook (doc §9) — a triangle is only tested
/// when `filter(tri.material)` is `true`. `cull` drops front- or back-facing
/// triangles to mirror the renderer's Culling menu ([`TriCull::None`] keeps the
/// game's two-sided behaviour). Returns the nearest passing triangle hit in
/// `(0, max_t]`, or `None`.
pub fn raycast_mesh(
  mesh: &CollisionMesh,
  ray: Ray,
  max_t: f32,
  filter: MaterialFilter<'_>,
  cull: TriCull,
) -> Option<RayHit> {
  let mut best_t = if max_t.is_finite() && max_t > 0.0 {
    max_t
  } else {
    f32::INFINITY
  };

  // The full per-triangle test, shared by both traversal strategies. Returns the
  // hit within `(0, hi)`, or `None` if the triangle is filtered, culled, or
  // missed.
  let test_tri = |idx: usize, hi: f32| -> Option<RayHit> {
    let tri = mesh.master_list_triangle(idx)?;
    if !filter(tri.material) {
      return None;
    }
    if !cull.keeps(mesh.render_tri_normal(idx), ray.dir) {
      return None;
    }
    let [v0, v1, v2] = tri.verts;
    let t = moller_trumbore_two_sided(ray.origin, ray.dir, v0, v1, v2, 0.0, hi)?;
    Some(RayHit {
      t,
      point: ray.origin + ray.dir * t,
      normal: (v1 - v0).cross(v2 - v0).normalize_or_zero(),
      tri_index: idx,
      material: tri.material,
    })
  };

  if let Some(bvh) = mesh.bvh.as_ref().filter(|b| !b.is_empty()) {
    let hit = bvh.nearest_hit(ray.origin, ray.dir, best_t, |prim, bound| {
      test_tri(prim as usize, bound).map(|h| h.t)
    })?;
    // Recompute the winning triangle's full hit (deterministic — same `t`).
    return test_tri(hit.prim as usize, f32::INFINITY);
  }

  let mut best: Option<RayHit> = None;
  for idx in 0..mesh.tri_count() {
    if let Some(hit) = test_tri(idx, best_t) {
      best_t = hit.t;
      best = Some(hit);
    }
  }
  best
}

/// `RayStaticIntersection` (`CGameCollision.cpp:249`) — cast against every area
/// mesh (each via its own BVH, see [`raycast_mesh`]) and keep the single nearest
/// hit. This is the static half of `RayWorldIntersection`; the dynamic half
/// (doc §8) is still deferred.
pub fn raycast_world<'a>(
  meshes: impl IntoIterator<Item = &'a CollisionMesh>,
  ray: Ray,
  max_t: f32,
  filter: MaterialFilter<'_>,
) -> Option<RayHit> {
  let mut best_t = if max_t.is_finite() && max_t > 0.0 {
    max_t
  } else {
    f32::INFINITY
  };
  let mut best: Option<RayHit> = None;
  for mesh in meshes {
    // Game-logic casts are two-sided, like the game's own octree.
    if let Some(hit) = raycast_mesh(mesh, ray, best_t, filter, TriCull::None)
      && hit.t < best_t
    {
      best_t = hit.t;
      best = Some(hit);
    }
  }
  best
}

#[cfg(test)]
mod tests {
  use super::*;

  /// verts (0,0,0)/(1,0,0)/(0,1,0), edges [0,1]/[1,2]/[2,0], one poly, one
  /// material — same shape as `collision_mesh`'s test helper.
  fn single_triangle(mat: CMaterialList) -> CollisionMesh {
    CollisionMesh {
      raw_verts: vec![
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
      ],
      raw_edges: vec![[0, 1], [1, 2], [2, 0]],
      raw_polys: vec![[0, 1, 2]],
      raw_poly_materials: vec![0],
      materials: vec![mat],
      ..Default::default()
    }
  }

  #[test]
  fn mt_hits_triangle_straight_on() {
    // Ray from z=+1 straight down at the centroid.
    let t = moller_trumbore_two_sided(
      Vec3::new(0.25, 0.25, 1.0),
      Vec3::new(0.0, 0.0, -1.0),
      Vec3::ZERO,
      Vec3::X,
      Vec3::Y,
      0.0,
      f32::INFINITY,
    );
    assert!((t.unwrap() - 1.0).abs() < 1e-5);
  }

  #[test]
  fn mt_is_two_sided() {
    // Same triangle, ray coming from below (z = -1, pointing up): back face.
    let t = moller_trumbore_two_sided(
      Vec3::new(0.25, 0.25, -1.0),
      Vec3::new(0.0, 0.0, 1.0),
      Vec3::ZERO,
      Vec3::X,
      Vec3::Y,
      0.0,
      f32::INFINITY,
    );
    assert!((t.unwrap() - 1.0).abs() < 1e-5);
  }

  #[test]
  fn mt_misses_outside_the_triangle() {
    let t = moller_trumbore_two_sided(
      Vec3::new(2.0, 2.0, 1.0),
      Vec3::new(0.0, 0.0, -1.0),
      Vec3::ZERO,
      Vec3::X,
      Vec3::Y,
      0.0,
      f32::INFINITY,
    );
    assert!(t.is_none());
  }

  #[test]
  fn mt_respects_the_t_window() {
    let o = Vec3::new(0.25, 0.25, 1.0);
    let d = Vec3::new(0.0, 0.0, -1.0);
    // hit is at t = 1.0
    assert!(moller_trumbore_two_sided(o, d, Vec3::ZERO, Vec3::X, Vec3::Y, 0.0, 0.5).is_none());
    assert!(moller_trumbore_two_sided(o, d, Vec3::ZERO, Vec3::X, Vec3::Y, 2.0, 5.0).is_none());
    assert!(moller_trumbore_two_sided(o, d, Vec3::ZERO, Vec3::X, Vec3::Y, 0.9, 1.1).is_some());
  }

  #[test]
  fn mt_parallel_ray_is_a_miss() {
    let t = moller_trumbore_two_sided(
      Vec3::new(0.25, 0.25, 1.0),
      Vec3::X,
      Vec3::ZERO,
      Vec3::X,
      Vec3::Y,
      0.0,
      f32::INFINITY,
    );
    assert!(t.is_none());
  }

  #[test]
  fn raycast_mesh_hits_the_single_triangle() {
    let mesh = single_triangle(CMaterialList(0));
    let hit = raycast_mesh(
      &mesh,
      Ray {
        origin: Vec3::new(0.2, 0.2, 5.0),
        dir: Vec3::new(0.0, 0.0, -1.0),
      },
      0.0,
      &pass_everything,
      TriCull::None,
    )
    .unwrap();
    assert_eq!(hit.tri_index, 0);
    assert!((hit.t - 5.0).abs() < 1e-4);
    assert!((hit.point - Vec3::new(0.2, 0.2, 0.0)).length() < 1e-4);
    assert!(hit.normal.dot(Vec3::Z).abs() > 0.99);
  }

  #[test]
  fn raycast_mesh_keeps_the_nearest_of_two_triangles() {
    // Two stacked triangles: z = 0 and z = 2. Ray from z = 5 downward hits z = 2 first.
    let mesh = CollisionMesh {
      raw_verts: vec![
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 2.0),
        Vec3::new(1.0, 0.0, 2.0),
        Vec3::new(0.0, 1.0, 2.0),
      ],
      raw_edges: vec![[0, 1], [1, 2], [2, 0], [3, 4], [4, 5], [5, 3]],
      raw_polys: vec![[0, 1, 2], [3, 4, 5]],
      raw_poly_materials: vec![0, 0],
      materials: vec![CMaterialList(0)],
      ..Default::default()
    };
    let hit = raycast_mesh(
      &mesh,
      Ray {
        origin: Vec3::new(0.2, 0.2, 5.0),
        dir: Vec3::new(0.0, 0.0, -1.0),
      },
      0.0,
      &pass_everything,
      TriCull::None,
    )
    .unwrap();
    assert_eq!(hit.tri_index, 1);
    assert!((hit.t - 3.0).abs() < 1e-4);
  }

  #[test]
  fn raycast_mesh_respects_max_t() {
    let mesh = single_triangle(CMaterialList(0));
    let ray = Ray {
      origin: Vec3::new(0.2, 0.2, 5.0),
      dir: Vec3::new(0.0, 0.0, -1.0),
    };
    assert!(raycast_mesh(&mesh, ray, 1.0, &pass_everything, TriCull::None).is_none());
    assert!(raycast_mesh(&mesh, ray, 10.0, &pass_everything, TriCull::None).is_some());
  }

  #[test]
  fn raycast_mesh_skips_filtered_out_triangles() {
    let mesh = single_triangle(CMaterialList::SOLID);
    let ray = Ray {
      origin: Vec3::new(0.2, 0.2, 5.0),
      dir: Vec3::new(0.0, 0.0, -1.0),
    };
    // reject everything -> miss; require SOLID -> hit
    assert!(raycast_mesh(&mesh, ray, 0.0, &|_| false, TriCull::None).is_none());
    assert!(
      raycast_mesh(
        &mesh,
        ray,
        0.0,
        &|m: CMaterialList| m.contains(CMaterialList::SOLID),
        TriCull::None,
      )
      .is_some()
    );
  }

  #[test]
  fn raycast_mesh_honours_tri_cull() {
    // A single triangle in the z = 0 plane. `build_vertices` gives it an
    // outward normal; a ray straight down (dir -Z) hits its front face.
    let mut mesh = single_triangle(CMaterialList(0));
    mesh.build_vertices();
    let n = mesh.render_tri_normal(0).unwrap();
    let ray = Ray {
      origin: Vec3::new(0.2, 0.2, 5.0),
      dir: Vec3::new(0.0, 0.0, -1.0),
    };
    let front_facing = n.dot(ray.dir) < 0.0;
    let (front, back) = if front_facing {
      (TriCull::FrontOnly, TriCull::BackOnly)
    } else {
      (TriCull::BackOnly, TriCull::FrontOnly)
    };
    assert!(raycast_mesh(&mesh, ray, 0.0, &pass_everything, front).is_some());
    assert!(raycast_mesh(&mesh, ray, 0.0, &pass_everything, back).is_none());
    assert!(raycast_mesh(&mesh, ray, 0.0, &pass_everything, TriCull::None).is_some());
  }

  #[test]
  fn raycast_world_keeps_the_nearest_across_meshes() {
    let far = single_triangle(CMaterialList(0)); // z = 0
    let mut near = single_triangle(CMaterialList(0));
    for v in &mut near.raw_verts {
      v.z += 2.0; // z = 2
    }
    let ray = Ray {
      origin: Vec3::new(0.2, 0.2, 5.0),
      dir: Vec3::new(0.0, 0.0, -1.0),
    };
    let hit = raycast_world([&far, &near], ray, 0.0, &pass_everything).unwrap();
    assert!((hit.t - 3.0).abs() < 1e-4);
  }

  /// A mesh with its BVH built must return the byte-identical hit the brute
  /// path returns — the index only changes which triangles get tested.
  #[test]
  fn raycast_mesh_bvh_matches_brute_force() {
    // A 10x10 grid of stacked triangles at varying heights.
    let mut mesh = CollisionMesh::default();
    let mut z = 0.0_f32;
    for gx in 0..10 {
      for gy in 0..10 {
        let base = mesh.raw_verts.len() as u16;
        let (x, y) = (gx as f32, gy as f32);
        z += 0.37;
        mesh.raw_verts.push(Vec3::new(x, y, z));
        mesh.raw_verts.push(Vec3::new(x + 1.0, y, z));
        mesh.raw_verts.push(Vec3::new(x, y + 1.0, z));
        mesh.raw_edges.push([base, base + 1]);
        mesh.raw_edges.push([base + 1, base + 2]);
        mesh.raw_edges.push([base + 2, base]);
        let e = (mesh.raw_edges.len() - 3) as u16;
        mesh.raw_polys.push([e, e + 1, e + 2]);
        mesh.raw_poly_materials.push(0);
      }
    }
    mesh.materials.push(CMaterialList(0));
    mesh.build_vertices();

    let brute = mesh.clone(); // bvh still None
    mesh.build_bvh();
    assert!(mesh.bvh.as_ref().is_some_and(|b| !b.is_empty()));

    for i in 0..40 {
      let ray = Ray {
        origin: Vec3::new(1.0 + (i as f32) * 0.2, 1.0 + (i as f32) * 0.15, 100.0),
        dir: Vec3::new(0.02, -0.01, -1.0).normalize(),
      };
      let a = raycast_mesh(&brute, ray, 0.0, &pass_everything, TriCull::None);
      let b = raycast_mesh(&mesh, ray, 0.0, &pass_everything, TriCull::None);
      match (a, b) {
        (None, None) => {}
        (Some(a), Some(b)) => {
          assert_eq!(a.tri_index, b.tri_index, "ray {i}");
          assert!((a.t - b.t).abs() < 1e-5, "ray {i}: {} vs {}", a.t, b.t);
        }
        (a, b) => panic!("ray {i}: brute {a:?} vs bvh {b:?}"),
      }
    }
  }
}
