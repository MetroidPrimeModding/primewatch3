//! Brute-force world ray cast — the static half of `CGameCollision::RayWorldIntersection`.
//!
//! See `src/world/collision_ray_tracing.md` for the full research notes. This is
//! the "recommended implementation" from §7: for each area's already-parsed
//! [`CollisionMesh`], reconstruct every master-list triangle (§5.2), run the
//! two-sided Möller–Trumbore test the octree uses (§5.1), and keep the nearest
//! hit. No octree traversal — a front-to-back octree descent and a min-`t` scan
//! return the same triangle for any ray that isn't grazing a node boundary at
//! almost exactly the hit distance, which does not matter for an inspection
//! overlay.
//!
//! The material filter (§9) is not implemented yet; every triangle passes. A
//! `CMaterialFilter` port slots in at the `filter_passes` call site.

use glam::Vec3;

use crate::world::collision_mesh::{CollisionMesh, ECollisionMaterial};

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
  pub material: ECollisionMaterial,
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

/// Brute-force the master triangle list of one area mesh (doc §7).
///
/// `max_t` bounds the search (world units); pass `<= 0.0` for an unbounded ray.
/// Returns the nearest triangle hit in `(0, max_t]`, or `None`.
pub fn raycast_mesh(mesh: &CollisionMesh, ray: Ray, max_t: f32) -> Option<RayHit> {
  let mut best_t = if max_t.is_finite() && max_t > 0.0 {
    max_t
  } else {
    f32::INFINITY
  };
  let mut best: Option<RayHit> = None;

  for idx in 0..mesh.tri_count() {
    let Some(tri) = mesh.master_list_triangle(idx) else {
      continue;
    };
    // TODO: CMaterialFilter (doc §9). For now every triangle passes.
    let [v0, v1, v2] = tri.verts;
    if let Some(t) = moller_trumbore_two_sided(ray.origin, ray.dir, v0, v1, v2, 0.0, best_t) {
      best_t = t;
      best = Some(RayHit {
        t,
        point: ray.origin + ray.dir * t,
        normal: (v1 - v0).cross(v2 - v0).normalize_or_zero(),
        tri_index: idx,
        material: tri.material,
      });
    }
  }

  best
}

#[cfg(test)]
mod tests {
  use super::*;

  /// verts (0,0,0)/(1,0,0)/(0,1,0), edges [0,1]/[1,2]/[2,0], one poly, one
  /// material — same shape as `collision_mesh`'s test helper.
  fn single_triangle(mat: ECollisionMaterial) -> CollisionMesh {
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
    let mesh = single_triangle(ECollisionMaterial(0));
    let hit = raycast_mesh(
      &mesh,
      Ray {
        origin: Vec3::new(0.2, 0.2, 5.0),
        dir: Vec3::new(0.0, 0.0, -1.0),
      },
      0.0,
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
      materials: vec![ECollisionMaterial(0)],
      ..Default::default()
    };
    let hit = raycast_mesh(
      &mesh,
      Ray {
        origin: Vec3::new(0.2, 0.2, 5.0),
        dir: Vec3::new(0.0, 0.0, -1.0),
      },
      0.0,
    )
    .unwrap();
    assert_eq!(hit.tri_index, 1);
    assert!((hit.t - 3.0).abs() < 1e-4);
  }

  #[test]
  fn raycast_mesh_respects_max_t() {
    let mesh = single_triangle(ECollisionMaterial(0));
    let ray = Ray {
      origin: Vec3::new(0.2, 0.2, 5.0),
      dir: Vec3::new(0.0, 0.0, -1.0),
    };
    assert!(raycast_mesh(&mesh, ray, 1.0).is_none());
    assert!(raycast_mesh(&mesh, ray, 10.0).is_some());
  }
}
