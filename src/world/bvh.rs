//! A small, self-contained bounding-volume hierarchy over triangle AABBs.
//!
//! This module knows nothing about `CollisionMesh`, materials, culling, or the
//! Möller–Trumbore test — it is a pure spatial index. You build it from a slice
//! of per-primitive [`Aabb`]s (primitive `id` = slice position) and query it
//! with [`Bvh::nearest_hit`], handing in a closure that runs the real
//! ray/primitive test. The tracer in [`crate::world::ray_trace`] is the only
//! caller; everything game-specific stays there.
//!
//! Build strategy is deliberately plain: an object-median split on the axis with
//! the largest centroid spread, four primitives per leaf. No SAH. That is enough
//! to turn the per-area brute-force scan (tens of thousands of triangles) into a
//! ~log-depth descent, which is all the interactive picker and the ball-camera
//! failsafe need.

use glam::Vec3;

#[derive(Clone, Copy, Debug)]
pub struct Aabb {
  pub min: Vec3,
  pub max: Vec3,
}

impl Aabb {
  pub const EMPTY: Aabb = Aabb {
    min: Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY),
    max: Vec3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY),
  };

  pub fn from_points(pts: impl IntoIterator<Item = Vec3>) -> Aabb {
    pts.into_iter().fold(Aabb::EMPTY, |b, p| b.expanded(p))
  }

  pub fn expanded(self, p: Vec3) -> Aabb {
    Aabb {
      min: self.min.min(p),
      max: self.max.max(p),
    }
  }

  pub fn union(self, other: Aabb) -> Aabb {
    Aabb {
      min: self.min.min(other.min),
      max: self.max.max(other.max),
    }
  }

  pub fn is_valid(&self) -> bool {
    self.min.cmple(self.max).all()
  }

  pub fn centroid(&self) -> Vec3 {
    (self.min + self.max) * 0.5
  }

  /// Slab test. `inv_dir` is the component-wise reciprocal of the (unit) ray
  /// direction. Returns the entry distance (clamped to `>= 0`, so an origin
  /// inside the box gives `0.0`) when the ray meets the box within
  /// `[0, max_t]`, else `None`.
  fn ray_entry(&self, origin: Vec3, inv_dir: Vec3, max_t: f32) -> Option<f32> {
    let t0 = (self.min - origin) * inv_dir;
    let t1 = (self.max - origin) * inv_dir;
    let enter = t0.min(t1).max_element().max(0.0);
    let exit = t0.max(t1).min_element();
    if enter <= exit && enter <= max_t {
      Some(enter)
    } else {
      None
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BvhHit {
  pub prim: u32,
  pub t: f32,
}

/// `count > 0` → leaf: `prims[first .. first + count]`.
/// `count == 0` → internal: children at node indices `first` (left) and
/// `right`.
#[derive(Clone, Copy, Debug)]
struct Node {
  bounds: Aabb,
  first: u32,
  right: u32,
  count: u32,
}

const LEAF_MAX: usize = 4;

#[derive(Clone, Debug, Default)]
pub struct Bvh {
  nodes: Vec<Node>,
  /// Primitive ids, grouped by leaf. A leaf's `first`/`count` index into this.
  prims: Vec<u32>,
}

impl Bvh {
  /// Build over `aabbs`, where primitive `id` is the index into `aabbs`.
  /// Primitives whose box is not [`valid`](Aabb::is_valid) are skipped, so an
  /// all-degenerate input yields an [`is_empty`](Bvh::is_empty) tree.
  pub fn build(aabbs: &[Aabb]) -> Bvh {
    let mut prims: Vec<u32> = (0..aabbs.len() as u32)
      .filter(|&i| aabbs[i as usize].is_valid())
      .collect();
    if prims.is_empty() {
      return Bvh::default();
    }

    let centroids: Vec<Vec3> = aabbs.iter().map(Aabb::centroid).collect();
    let mut nodes = Vec::with_capacity(2 * prims.len() / LEAF_MAX + 1);
    build_node(&mut nodes, &mut prims, 0, aabbs, &centroids);
    Bvh { nodes, prims }
  }

  pub fn is_empty(&self) -> bool {
    self.nodes.is_empty()
  }

  /// Nearest primitive the ray hits within `(0, max_t]`.
  ///
  /// `dir` is expected to be unit length. Pass `max_t <= 0` (or infinite) for
  /// an unbounded ray. `test(prim_id, bound)` runs the caller's real
  /// ray/primitive intersection and returns `Some(t)` when the primitive is hit
  /// with `0 < t < bound`, else `None`; the traversal tightens `bound` as it
  /// finds closer hits, so leaves past the current best are pruned by the slab
  /// test.
  pub fn nearest_hit<F>(&self, origin: Vec3, dir: Vec3, max_t: f32, mut test: F) -> Option<BvhHit>
  where
    F: FnMut(u32, f32) -> Option<f32>,
  {
    if self.nodes.is_empty() {
      return None;
    }
    let inv_dir = Vec3::ONE / dir;
    let mut best_t = if max_t.is_finite() && max_t > 0.0 {
      max_t
    } else {
      f32::INFINITY
    };
    let mut best: Option<BvhHit> = None;

    // Explicit stack; grows past 64 only for a pathologically deep tree.
    let mut stack: Vec<u32> = Vec::with_capacity(64);
    stack.push(0);
    while let Some(ni) = stack.pop() {
      let node = &self.nodes[ni as usize];
      // Re-check against the (possibly shrunk) bound at pop time.
      if node.bounds.ray_entry(origin, inv_dir, best_t).is_none() {
        continue;
      }

      if node.count > 0 {
        let lo = node.first as usize;
        for &prim in &self.prims[lo..lo + node.count as usize] {
          if let Some(t) = test(prim, best_t)
            && t < best_t
          {
            best_t = t;
            best = Some(BvhHit { prim, t });
          }
        }
        continue;
      }

      // Internal: descend the nearer child first, and only queue a child whose
      // box the ray still reaches inside the current best distance.
      let l = node.first;
      let r = node.right;
      let lt = self.nodes[l as usize]
        .bounds
        .ray_entry(origin, inv_dir, best_t);
      let rt = self.nodes[r as usize]
        .bounds
        .ray_entry(origin, inv_dir, best_t);
      match (lt, rt) {
        (None, None) => {}
        (Some(_), None) => stack.push(l),
        (None, Some(_)) => stack.push(r),
        (Some(a), Some(b)) => {
          // push far, then near, so near is popped first
          if a <= b {
            stack.push(r);
            stack.push(l);
          } else {
            stack.push(l);
            stack.push(r);
          }
        }
      }
    }

    best
  }
}

/// Recursively append the subtree for `prims` (an owned window into the shared
/// prim array, whose global start is `offset`) and return its node index.
fn build_node(
  nodes: &mut Vec<Node>,
  prims: &mut [u32],
  offset: u32,
  aabbs: &[Aabb],
  centroids: &[Vec3],
) -> u32 {
  let bounds = prims
    .iter()
    .fold(Aabb::EMPTY, |b, &p| b.union(aabbs[p as usize]));

  let leaf = |nodes: &mut Vec<Node>| -> u32 {
    let idx = nodes.len() as u32;
    nodes.push(Node {
      bounds,
      first: offset,
      right: 0,
      count: prims.len() as u32,
    });
    idx
  };

  if prims.len() <= LEAF_MAX {
    return leaf(nodes);
  }

  // Split axis = largest spread of centroids. If the centroids coincide there is
  // nothing to separate — keep them together as one (oversized) leaf.
  let cbounds = prims
    .iter()
    .fold(Aabb::EMPTY, |b, &p| b.expanded(centroids[p as usize]));
  let extent = cbounds.max - cbounds.min;
  let axis = largest_axis(extent);
  if extent[axis] <= f32::EPSILON {
    return leaf(nodes);
  }

  // Object median: partition around the middle element by centroid on `axis`.
  let mid = prims.len() / 2;
  prims.select_nth_unstable_by(mid, |&a, &b| {
    centroids[a as usize][axis]
      .partial_cmp(&centroids[b as usize][axis])
      .unwrap_or(std::cmp::Ordering::Equal)
  });

  let me = nodes.len() as u32;
  nodes.push(Node {
    bounds,
    first: 0,
    right: 0,
    count: 0,
  }); // placeholder, patched below

  let (left, right) = prims.split_at_mut(mid);
  let l = build_node(nodes, left, offset, aabbs, centroids);
  let r = build_node(nodes, right, offset + mid as u32, aabbs, centroids);
  nodes[me as usize] = Node {
    bounds,
    first: l,
    right: r,
    count: 0,
  };
  me
}

fn largest_axis(v: Vec3) -> usize {
  if v.x >= v.y && v.x >= v.z {
    0
  } else if v.y >= v.z {
    1
  } else {
    2
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::world::ray_trace::moller_trumbore_two_sided;

  /// xorshift64 — deterministic, no dev-dependency.
  struct Rng(u64);
  impl Rng {
    fn u32(&mut self) -> u32 {
      self.0 ^= self.0 << 13;
      self.0 ^= self.0 >> 7;
      self.0 ^= self.0 << 17;
      (self.0 >> 32) as u32
    }
    fn unit(&mut self) -> f32 {
      self.u32() as f32 / u32::MAX as f32
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
      a + (b - a) * self.unit()
    }
    fn vec(&mut self, a: f32, b: f32) -> Vec3 {
      Vec3::new(self.range(a, b), self.range(a, b), self.range(a, b))
    }
  }

  fn scene(rng: &mut Rng, n: usize) -> Vec<[Vec3; 3]> {
    (0..n)
      .map(|_| {
        let c = rng.vec(-50.0, 50.0);
        [
          c + rng.vec(-3.0, 3.0),
          c + rng.vec(-3.0, 3.0),
          c + rng.vec(-3.0, 3.0),
        ]
      })
      .collect()
  }

  fn hit_tri(o: Vec3, d: Vec3, tri: &[Vec3; 3], hi: f32) -> Option<f32> {
    moller_trumbore_two_sided(o, d, tri[0], tri[1], tri[2], 0.0, hi)
  }

  fn brute_nearest(o: Vec3, d: Vec3, tris: &[[Vec3; 3]]) -> Option<f32> {
    let mut best = f32::INFINITY;
    for tri in tris {
      if let Some(t) = hit_tri(o, d, tri, best) {
        best = t;
      }
    }
    best.is_finite().then_some(best)
  }

  #[test]
  fn empty_input_builds_empty_tree() {
    let bvh = Bvh::build(&[]);
    assert!(bvh.is_empty());
    assert!(
      bvh
        .nearest_hit(Vec3::ZERO, Vec3::X, 100.0, |_, _| Some(1.0))
        .is_none()
    );
  }

  #[test]
  fn degenerate_boxes_are_dropped() {
    let bvh = Bvh::build(&[Aabb::EMPTY, Aabb::EMPTY]);
    assert!(bvh.is_empty());
  }

  #[test]
  fn matches_brute_force_over_many_rays() {
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let tris = scene(&mut rng, 600);
    let aabbs: Vec<Aabb> = tris
      .iter()
      .map(|t| Aabb::from_points(t.iter().copied()))
      .collect();
    let bvh = Bvh::build(&aabbs);
    assert!(!bvh.is_empty());

    let mut checked = 0;
    for _ in 0..2000 {
      let o = rng.vec(-90.0, 90.0);
      let d = rng.vec(-1.0, 1.0);
      if d.length() < 1e-3 {
        continue;
      }
      let d = d.normalize();

      let brute = brute_nearest(o, d, &tris);
      let via_bvh = bvh
        .nearest_hit(o, d, f32::INFINITY, |p, bound| {
          hit_tri(o, d, &tris[p as usize], bound)
        })
        .map(|h| h.t);

      match (brute, via_bvh) {
        (None, None) => {}
        (Some(a), Some(b)) => {
          assert!((a - b).abs() < 1e-2, "brute {a} vs bvh {b}");
          checked += 1;
        }
        (a, b) => panic!("hit/miss disagreement: brute {a:?} vs bvh {b:?}"),
      }
    }
    assert!(checked > 50, "test scene produced too few hits ({checked})");
  }

  #[test]
  fn respects_max_t() {
    // One triangle straddling the origin plane at z = 0, ray from z = 10 down.
    let tri = [
      Vec3::new(-1.0, -1.0, 0.0),
      Vec3::new(1.0, -1.0, 0.0),
      Vec3::new(0.0, 1.0, 0.0),
    ];
    let aabbs = [Aabb::from_points(tri.iter().copied())];
    let bvh = Bvh::build(&aabbs);
    let o = Vec3::new(0.0, 0.0, 10.0);
    let d = Vec3::new(0.0, 0.0, -1.0);
    let test = |p: u32, bound: f32| hit_tri(o, d, &tri, bound.min(f32::MAX)).filter(|_| p == 0);

    assert!(bvh.nearest_hit(o, d, 5.0, test).is_none()); // hit is at t = 10
    let h = bvh.nearest_hit(o, d, 20.0, test).unwrap();
    assert_eq!(h.prim, 0);
    assert!((h.t - 10.0).abs() < 1e-4);
  }
}
