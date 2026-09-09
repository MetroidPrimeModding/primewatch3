//! The memory -> struct parse ([`load_mesh`]) and the triangle-soup build
//! ([`CollisionMesh::build_vertices`]). No GPU code lives here.

use glam::{Vec3, Vec4};

use crate::ctx::Ctx;
use crate::gl::Vert;
use crate::gl::shapes;
use crate::structs::prime_structs::GameInstance;
use crate::world::bvh::{Aabb, Bvh};

/// Collision-surface material bitflags (mirrored by `enum CollisionMaterial` in
/// `prime_defs/prime1/CAreaOctTree.bs`).
///
/// A native bitflag newtype rather than a `GameEnum`: the game ORs these
/// together and tests them with `!!(a & b)`. [`contains`] is that idiom.
///
/// [`contains`]: ECollisionMaterial::contains
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ECollisionMaterial(pub u32);

impl std::fmt::Debug for ECollisionMaterial {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "ECollisionMaterial({:#010x})", self.0)
  }
}

#[allow(unused)]
impl ECollisionMaterial {
  pub const UNKNOWN_1: ECollisionMaterial = ECollisionMaterial(0x1);
  pub const STONE: ECollisionMaterial = ECollisionMaterial(0x2);
  pub const METAL: ECollisionMaterial = ECollisionMaterial(0x4);
  pub const GRASS: ECollisionMaterial = ECollisionMaterial(0x8);
  pub const ICE: ECollisionMaterial = ECollisionMaterial(0x10);
  pub const PILLAR: ECollisionMaterial = ECollisionMaterial(0x20);
  pub const METAL_GRATING: ECollisionMaterial = ECollisionMaterial(0x40);
  pub const PHAZON: ECollisionMaterial = ECollisionMaterial(0x80);
  pub const DIRT: ECollisionMaterial = ECollisionMaterial(0x100);
  pub const LAVA: ECollisionMaterial = ECollisionMaterial(0x200);
  pub const UNKNOWN_2: ECollisionMaterial = ECollisionMaterial(0x400);
  pub const SNOW: ECollisionMaterial = ECollisionMaterial(0x800);
  pub const SLOW_MUD: ECollisionMaterial = ECollisionMaterial(0x1000);
  pub const HALFPIPE: ECollisionMaterial = ECollisionMaterial(0x2000);
  pub const MUD: ECollisionMaterial = ECollisionMaterial(0x4000);
  pub const GLASS: ECollisionMaterial = ECollisionMaterial(0x8000);
  pub const SHIELD: ECollisionMaterial = ECollisionMaterial(0x10000);
  pub const SAND: ECollisionMaterial = ECollisionMaterial(0x20000);
  pub const SHOOT_THRU: ECollisionMaterial = ECollisionMaterial(0x40000);
  pub const SOLID: ECollisionMaterial = ECollisionMaterial(0x80000);
  pub const UNKNOWN_3: ECollisionMaterial = ECollisionMaterial(0x100000);
  pub const CAMERA_THRU: ECollisionMaterial = ECollisionMaterial(0x200000);
  pub const WOOD: ECollisionMaterial = ECollisionMaterial(0x400000);
  pub const ORGANIC: ECollisionMaterial = ECollisionMaterial(0x800000);
  pub const UNKNOWN_4: ECollisionMaterial = ECollisionMaterial(0x1000000);
  pub const REDUNDANT_EDGE: ECollisionMaterial = ECollisionMaterial(0x2000000);
  pub const FLIPPED_TRI: ECollisionMaterial = ECollisionMaterial(0x2000000);
  pub const SEE_THRU: ECollisionMaterial = ECollisionMaterial(0x4000000);
  pub const SCAN_THRU: ECollisionMaterial = ECollisionMaterial(0x8000000);
  pub const AI_WALK_THRU: ECollisionMaterial = ECollisionMaterial(0x10000000);
  pub const CEILING: ECollisionMaterial = ECollisionMaterial(0x20000000);
  pub const WALL: ECollisionMaterial = ECollisionMaterial(0x40000000);
  pub const FLOOR: ECollisionMaterial = ECollisionMaterial(0x80000000);

  /// Ports the C++ `!!(a & b)` idiom — is any bit of `flag` set in `self`?
  pub fn contains(self, flag: ECollisionMaterial) -> bool {
    (self.0 & flag.0) != 0
  }
}

/// CPU-side collision geometry.
///
/// `raw_*` are the arrays copied straight out of the game's `CAreaOctTree`;
/// [`build_vertices`] resolves them into [`verts`], the triangle soup the
/// renderer uploads.
///
/// [`build_vertices`]: CollisionMesh::build_vertices
/// [`verts`]: CollisionMesh::verts
#[derive(Default, Clone)]
pub struct CollisionMesh {
  pub raw_verts: Vec<Vec3>,
  pub raw_vert_materials: Vec<u16>,
  pub raw_edges: Vec<[u16; 2]>,
  pub raw_edge_materials: Vec<u16>,
  pub raw_polys: Vec<[u16; 3]>,
  pub raw_poly_materials: Vec<u16>,
  pub min: Vec3,
  pub max: Vec3,
  pub materials: Vec<ECollisionMaterial>,
  /// Filled by [`CollisionMesh::build_vertices`] — the tri soup the renderer uploads.
  pub verts: Vec<Vert>,
  /// Spatial index over the master triangle list, built by
  /// [`CollisionMesh::build_bvh`]. `None` until built (test fixtures, the
  /// default mesh); the ray tracer brute-forces when it is absent.
  pub bvh: Option<Bvh>,
}

/// Read `x` / `y` / `z` `f32` members off a `CVector3f`-shaped handle.
fn read_cvector3f(ctx: &Ctx, m: &GameInstance) -> Vec3 {
  Vec3::new(
    m.member(ctx, "x").read_f32(ctx).unwrap_or(0.0),
    m.member(ctx, "y").read_f32(ctx).unwrap_or(0.0),
    m.member(ctx, "z").read_f32(ctx).unwrap_or(0.0),
  )
}

/// Defensive sanity cap on each of the four `CAreaOctTree` counts.
const COUNT_SANITY_CAP: u32 = 50_000;

/// Walks `area -> postConstructed -> collision["value"]` (a `*CAreaOctTree`),
/// copies its material / vertex / edge / poly arrays out of game memory, records
/// the area AABB, then runs [`CollisionMesh::build_vertices`].
///
/// Returns `None` on a structural miss (missing member, null `collision`,
/// unreadable count, an out-of-range count). The bulk array reads use
/// `.unwrap_or(0)` / `.unwrap_or(0.0)` - preventing panics.
pub fn load_mesh(ctx: &Ctx, area: &GameInstance) -> Option<CollisionMesh> {
  // 1. `*CPostConstructed` (auto-deref'd by `get_member`).
  let post_constructed = area.get_member(ctx, "postConstructed")?;

  // 2. `collision` (`rstl::autoptr<CAreaOctTree>` inline) -> `["value"]`
  //    (auto-derefs `*CAreaOctTree`).
  let collision = post_constructed
    .get_member(ctx, "collision")?
    .get_member(ctx, "value")?;
  if collision.address == 0 {
    return None;
  }

  let mut res = CollisionMesh::default();

  // 3. Element counts + sanity gate.
  let mat_count = collision.get_member(ctx, "matCount")?.read_u32(ctx)?;
  let edge_count = collision.get_member(ctx, "edgeCount")?.read_u32(ctx)?;
  let poly_count = collision.get_member(ctx, "polyCount")?.read_u32(ctx)?;
  let vert_count = collision.get_member(ctx, "vertCount")?.read_u32(ctx)?;

  if mat_count > COUNT_SANITY_CAP
    || edge_count > COUNT_SANITY_CAP
    || poly_count > COUNT_SANITY_CAP
    || vert_count > COUNT_SANITY_CAP
  {
    eprintln!("Bad read for polys");
    return None;
  }

  // 4. Array base addresses. `get_member` auto-derefs the pointer members, so
  //    `.address` is the array base.
  let material_start = collision.get_member(ctx, "materials")?.address;
  let edge_start = collision.get_member(ctx, "edges")?.address;
  let poly_start = collision.get_member(ctx, "polyEdges")?.address;
  let vert_start = collision.get_member(ctx, "verts")?.address;

  // TODO: the C++ `WorldRenderer::loadMesh` reads the per-vertex materials from
  // the `polyEdges` pointer, not `vertMats` — an apparent bug in the original.
  // Preserved verbatim; `raw_vert_materials` is unused by `build_vertices`.
  let vert_material_start = collision.get_member(ctx, "polyEdges")?.address;
  let edge_material_start = collision.get_member(ctx, "edgeMats")?.address;
  let poly_material_start = collision.get_member(ctx, "polyMats")?.address;

  let mem = ctx.mem;

  // 5. Fill the raw vecs. Addresses pass straight through — `GameMemory` masks
  //    `& 0x7FFFFFFF` and byte-swaps internally; do not re-do either here.
  for i in 0..mat_count {
    let a = material_start.wrapping_add(i.wrapping_mul(4));
    res
      .materials
      .push(ECollisionMaterial(mem.read_u32(a).unwrap_or(0)));
  }

  for i in 0..vert_count {
    let base = vert_start.wrapping_add(i.wrapping_mul(12));
    res.raw_verts.push(Vec3::new(
      mem.read_f32(base).unwrap_or(0.0),
      mem.read_f32(base.wrapping_add(4)).unwrap_or(0.0),
      mem.read_f32(base.wrapping_add(8)).unwrap_or(0.0),
    ));
  }
  // separate loop for locality reasons
  for i in 0..vert_count {
    res.raw_vert_materials.push(
      mem
        .read_u8(vert_material_start.wrapping_add(i))
        .unwrap_or(0) as u16,
    );
  }

  for i in 0..edge_count {
    let base = edge_start.wrapping_add(i.wrapping_mul(4));
    res.raw_edges.push([
      mem.read_u16(base).unwrap_or(0),
      mem.read_u16(base.wrapping_add(2)).unwrap_or(0),
    ]);
  }
  for i in 0..edge_count {
    res.raw_edge_materials.push(
      mem
        .read_u8(edge_material_start.wrapping_add(i))
        .unwrap_or(0) as u16,
    );
  }

  for i in 0..poly_count {
    let base = poly_start.wrapping_add(i.wrapping_mul(6));
    res.raw_polys.push([
      mem.read_u16(base).unwrap_or(0),
      mem.read_u16(base.wrapping_add(2)).unwrap_or(0),
      mem.read_u16(base.wrapping_add(4)).unwrap_or(0),
    ]);
  }
  for i in 0..poly_count {
    res.raw_poly_materials.push(
      mem
        .read_u8(poly_material_start.wrapping_add(i))
        .unwrap_or(0) as u16,
    );
  }

  // 6. AABB. `CGameArea.aabb` is a `CAABB` at 0x6C -> `CVector3f min/max`.
  let aabb = area.member(ctx, "aabb");
  res.min = read_cvector3f(ctx, &aabb.member(ctx, "min"));
  res.max = read_cvector3f(ctx, &aabb.member(ctx, "max"));

  // 7.
  res.build_vertices();
  res.build_bvh();
  Some(res)
}

/// One reconstructed master-list triangle: its 3 world-space verts (winding
/// already resolved) and its surface material word.
#[derive(Clone, Copy, Debug)]
pub struct MasterTri {
  pub verts: [Vec3; 3],
  pub material: ECollisionMaterial,
}

impl CollisionMesh {
  pub fn tri_count(&self) -> usize {
    self.raw_polys.len()
  }

  /// Reconstruct triangle `idx` exactly as the game's `GetMasterListTriangle`
  /// does (`CAreaOctTree.cpp:591-604`, doc §5.2): pick the 3 verts from the
  /// **first two** of the triangle's edges, and swap the first two verts when
  /// the surface word has bit `0x0200_0000` (`FLIPPED_TRI`).
  ///
  /// This is deliberately *not* the 3-edge walk [`build_vertices`] uses for
  /// rendering — the ray tracer replicates the game's own reconstruction so a
  /// hit's winding/normal matches what the game's collision would report.
  ///
  /// Returns `None` if any edge or vertex index is out of range.
  ///
  /// [`build_vertices`]: CollisionMesh::build_vertices
  pub fn master_list_triangle(&self, idx: usize) -> Option<MasterTri> {
    let poly_edges = *self.raw_polys.get(idx)?;
    let e0 = *self.raw_edges.get(poly_edges[0] as usize)?;
    let e1 = *self.raw_edges.get(poly_edges[1] as usize)?;

    // vert2 = the endpoint of e1 that isn't shared with e0.
    let vert2 = if e1[0] != e0[0] && e1[0] != e0[1] {
      e1[0]
    } else {
      e1[1]
    };

    let material = self
      .raw_poly_materials
      .get(idx)
      .and_then(|&m| self.materials.get(m as usize))
      .copied()
      .unwrap_or(ECollisionMaterial(0));

    let vert = |i: u16| self.raw_verts.get(i as usize).copied();
    let (a, b) = if material.contains(ECollisionMaterial::FLIPPED_TRI) {
      (vert(e0[1])?, vert(e0[0])?)
    } else {
      (vert(e0[0])?, vert(e0[1])?)
    };
    let c = vert(vert2)?;

    Some(MasterTri {
      verts: [a, b, c],
      material,
    })
  }

  pub fn render_tri_normal(&self, idx: usize) -> Option<Vec3> {
    self.verts.get(idx * 3).map(|v| Vec3::from_array(v.normal))
  }

  /// Builds [`CollisionMesh::bvh`] from the master triangle list
  pub fn build_bvh(&mut self) {
    let aabbs: Vec<Aabb> = (0..self.tri_count())
      .map(|i| match self.master_list_triangle(i) {
        Some(tri) => Aabb::from_points(tri.verts),
        None => Aabb::EMPTY,
      })
      .collect();
    self.bvh = Some(Bvh::build(&aabbs));
  }

  /// Fills [`CollisionMesh::verts`]
  pub fn build_vertices(&mut self) {
    let mut verts: Vec<Vert> = Vec::with_capacity(self.raw_polys.len() * 3);

    for (i, edges) in self.raw_polys.iter().enumerate() {
      let poly_mat_idx = self.raw_poly_materials.get(i).copied().unwrap_or(0) as usize;
      let poly_flags = self
        .materials
        .get(poly_mat_idx)
        .copied()
        .unwrap_or(ECollisionMaterial(0));

      let line1 = self
        .raw_edges
        .get(edges[0] as usize)
        .copied()
        .unwrap_or_default();
      let line2 = self
        .raw_edges
        .get(edges[1] as usize)
        .copied()
        .unwrap_or_default();
      let line3 = self
        .raw_edges
        .get(edges[2] as usize)
        .copied()
        .unwrap_or_default();

      // point 1
      let mut i1 = line1[0];

      // point 2
      let (i2, other_line) = if line1[0] == line2[0] {
        (line2[1], line3)
      } else if line1[0] == line2[1] {
        (line2[0], line3)
      } else if line1[0] == line3[0] {
        (line3[1], line2)
      } else {
        (line3[0], line2)
      };

      // point 3
      let mut i3 = if i2 == other_line[0] {
        other_line[1]
      } else {
        other_line[0]
      };

      // swap if needed
      if poly_flags.contains(ECollisionMaterial::FLIPPED_TRI) {
        std::mem::swap(&mut i1, &mut i3);
      }

      let v1 = self.raw_verts.get(i1 as usize).copied().unwrap_or_default();
      let v2 = self.raw_verts.get(i2 as usize).copied().unwrap_or_default();
      let v3 = self.raw_verts.get(i3 as usize).copied().unwrap_or_default();

      let n = (v1 - v3).cross(v1 - v2).normalize();

      let color = surface_color(poly_flags, n);

      verts.push(Vert {
        pos: v1.to_array(),
        color,
        normal: n.to_array(),
        barycentric: [1.0, 0.0, 0.0],
      });
      verts.push(Vert {
        pos: v2.to_array(),
        color,
        normal: n.to_array(),
        barycentric: [0.0, 1.0, 0.0],
      });
      verts.push(Vert {
        pos: v3.to_array(),
        color,
        normal: n.to_array(),
        barycentric: [0.0, 0.0, 1.0],
      });
    }

    self.verts = verts;
  }
}

/// The game's standability tint for a collision surface, from its material
/// flags and world-space normal — `WorldRenderer::loadMesh`'s colour ladder.
///
/// The redundant `|| normal.z > 0.85` on all three arms is a verbatim C++
/// quirk: in practice only a `FLOOR` flag (or a steeply upward normal, which
/// takes the first arm) ever leaves the default grey, so walls/ceilings show
/// grey unless their flag is set.
pub fn surface_color(flags: ECollisionMaterial, normal: Vec3) -> [f32; 4] {
  if flags.contains(ECollisionMaterial::FLOOR) || normal.z > 0.85 {
    [0.4, 0.6, 0.4, 1.0]
  } else if flags.contains(ECollisionMaterial::WALL) || normal.z > 0.85 {
    [0.6, 0.6, 0.6, 1.0]
  } else if flags.contains(ECollisionMaterial::CEILING) || normal.z > 0.85 {
    [0.8, 0.5, 0.5, 1.0]
  } else {
    [0.2, 0.2, 0.2, 1.0]
  }
}

/// Standability tint from an axis-aligned face normal alone, for geometry that
/// carries no material flags (a `CPhysicsActor`'s AABox primitive).
///
/// [`surface_color`] can't be used here: its verbatim `|| n.z > 0.85` quirk
/// makes the wall and ceiling arms unreachable without a flag, so every
/// non-top face falls through to the near-black `0.2` default. This picks the
/// same three tints geometrically instead, so side faces read as walls.
fn box_face_color(normal: Vec3) -> [f32; 4] {
  if normal.z > 0.85 {
    [0.4, 0.6, 0.4, 1.0] // floor
  } else if normal.z < -0.85 {
    [0.8, 0.5, 0.5, 1.0] // ceiling
  } else {
    [0.6, 0.6, 0.6, 1.0] // wall
  }
}

/// A collision-styled axis-aligned box: [`shapes::generate_cube`] geometry
/// (per-face normals + barycentric wireframe intact) recoloured face-by-face
/// with [`box_face_color`] — so a `CPhysicsActor`'s AABox collision primitive
/// draws with the same look as the area mesh.
pub fn collision_box_verts(min: Vec3, max: Vec3) -> Vec<Vert> {
  let mut verts = shapes::generate_cube(min, max, Vec4::ONE);
  for v in &mut verts {
    v.color = box_face_color(Vec3::from_array(v.normal));
  }
  verts
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::mem::area_utils::get_areas;
  use crate::mem::game_memory::GameMemory;
  use crate::structs::prime_structs::GameStructs;

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

  fn norm_len(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
  }

  #[test]
  fn surface_color_arms() {
    // Steep upward normal -> floor tint via the first arm, no flag needed.
    assert_eq!(
      surface_color(ECollisionMaterial(0), Vec3::Z),
      [0.4, 0.6, 0.4, 1.0]
    );
    // Flat normal, no flags -> default grey. The verbatim `|| n.z > 0.85` quirk
    // makes the WALL / CEILING arms unreachable without their flag.
    assert_eq!(
      surface_color(ECollisionMaterial(0), Vec3::X),
      [0.2, 0.2, 0.2, 1.0]
    );
    assert_eq!(
      surface_color(ECollisionMaterial::WALL, Vec3::X),
      [0.6, 0.6, 0.6, 1.0]
    );
    assert_eq!(
      surface_color(ECollisionMaterial::CEILING, Vec3::NEG_Z),
      [0.8, 0.5, 0.5, 1.0]
    );
  }

  #[test]
  fn collision_box_verts_tints_each_face_by_orientation() {
    let verts = collision_box_verts(Vec3::splat(-1.0), Vec3::splat(1.0));
    assert_eq!(verts.len(), 36);
    for v in &verts {
      let want = if v.normal[2] > 0.85 {
        [0.4, 0.6, 0.4, 1.0] // floor
      } else if v.normal[2] < -0.85 {
        [0.8, 0.5, 0.5, 1.0] // ceiling
      } else {
        [0.6, 0.6, 0.6, 1.0] // wall — never the near-black default
      };
      assert_eq!(v.color, want);
    }
    // 6 verts up, 6 down, 24 on the four side faces.
    assert_eq!(verts.iter().filter(|v| v.normal[2] > 0.85).count(), 6);
    assert_eq!(verts.iter().filter(|v| v.normal[2] < -0.85).count(), 6);
    assert_eq!(
      verts.iter().filter(|v| v.normal[2].abs() <= 0.85).count(),
      24
    );
  }

  #[test]
  fn build_vertices_single_triangle() {
    let mut mesh = single_triangle(ECollisionMaterial(0));
    mesh.build_vertices();

    assert_eq!(mesh.verts.len(), 3);

    // Edge walk resolves i1=0, i2=2, i3=1 -> v1=(0,0,0) v2=(0,1,0) v3=(1,0,0).
    assert_eq!(mesh.verts[0].pos, [0.0, 0.0, 0.0]);
    assert_eq!(mesh.verts[1].pos, [0.0, 1.0, 0.0]);
    assert_eq!(mesh.verts[2].pos, [1.0, 0.0, 0.0]);

    assert_eq!(mesh.verts[0].barycentric, [1.0, 0.0, 0.0]);
    assert_eq!(mesh.verts[1].barycentric, [0.0, 1.0, 0.0]);
    assert_eq!(mesh.verts[2].barycentric, [0.0, 0.0, 1.0]);

    // n = normalize((v1-v3) x (v1-v2)) = (0,0,1): unit length, shared by all 3.
    for v in &mesh.verts {
      assert!((norm_len(v.normal) - 1.0).abs() < 1e-5);
      assert_eq!(v.normal, mesh.verts[0].normal);
    }
    assert!((mesh.verts[0].normal[2].abs() - 1.0).abs() < 1e-5);
  }

  #[test]
  fn build_vertices_flipped_tri_swaps_v1_v3() {
    let mut plain = single_triangle(ECollisionMaterial(0));
    plain.build_vertices();
    let mut flipped = single_triangle(ECollisionMaterial::FLIPPED_TRI);
    flipped.build_vertices();

    assert_eq!(flipped.verts.len(), 3);
    // v1 <-> v3 swapped; v2 untouched.
    assert_eq!(flipped.verts[0].pos, plain.verts[2].pos);
    assert_eq!(flipped.verts[2].pos, plain.verts[0].pos);
    assert_eq!(flipped.verts[1].pos, plain.verts[1].pos);
  }

  #[test]
  fn master_list_triangle_uses_first_two_edges() {
    let mesh = single_triangle(ECollisionMaterial(0));
    let tri = mesh.master_list_triangle(0).unwrap();
    // e0 = [0,1] -> verts[0], verts[1]; e1 = [1,2] -> unshared endpoint is 2.
    assert_eq!(tri.verts[0], Vec3::new(0.0, 0.0, 0.0));
    assert_eq!(tri.verts[1], Vec3::new(1.0, 0.0, 0.0));
    assert_eq!(tri.verts[2], Vec3::new(0.0, 1.0, 0.0));
  }

  #[test]
  fn master_list_triangle_flipped_swaps_first_two() {
    let plain = single_triangle(ECollisionMaterial(0))
      .master_list_triangle(0)
      .unwrap();
    let flipped = single_triangle(ECollisionMaterial::FLIPPED_TRI)
      .master_list_triangle(0)
      .unwrap();
    assert_eq!(flipped.verts[0], plain.verts[1]);
    assert_eq!(flipped.verts[1], plain.verts[0]);
    assert_eq!(flipped.verts[2], plain.verts[2]);
  }

  #[test]
  fn master_list_triangle_out_of_range_is_none() {
    let mesh = single_triangle(ECollisionMaterial(0));
    assert!(mesh.master_list_triangle(1).is_none());
  }

  #[test]
  fn collision_material_contains_matches_cpp_idiom() {
    let m = ECollisionMaterial(ECollisionMaterial::FLOOR.0 | ECollisionMaterial::WALL.0);
    assert!(m.contains(ECollisionMaterial::FLOOR));
    assert!(m.contains(ECollisionMaterial::WALL));
    assert!(!m.contains(ECollisionMaterial::CEILING));
    // REDUNDANT_EDGE and FLIPPED_TRI alias 0x2000000, verbatim from C++.
    assert_eq!(
      ECollisionMaterial::REDUNDANT_EDGE.0,
      ECollisionMaterial::FLIPPED_TRI.0
    );
  }

  /// Real `.bs` schema from this crate's `prime_defs/` (same loader as
  /// `area_utils.rs` tests).
  fn load_defs() -> GameStructs {
    let mut structs = GameStructs::new_empty();
    structs
      .load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/prime_defs"))
      .expect("load prime_defs");
    structs
  }

  /// Skip-if-absent loader for the offline BE dump.
  fn load_mem1() -> Option<GameMemory> {
    let path = std::env::var("PRIMEWATCH_MEM1_RAW")
      .unwrap_or_else(|_| format!("{}/mem1.raw", env!("CARGO_MANIFEST_DIR")));
    if !std::path::Path::new(&path).exists() {
      eprintln!("skipping collision_mesh mem1.raw test: {path} not found");
      return None;
    }
    let mut mem = GameMemory::new();
    mem.load_from_file(&path).expect("read mem1.raw");
    Some(mem)
  }

  #[test]
  fn load_mesh_over_live_areas() {
    let Some(mem) = load_mem1() else { return };
    let structs = load_defs();
    let ctx = Ctx::new(&structs, &mem);

    let areas = get_areas(&ctx);
    let mut loaded = 0;
    for area in &areas {
      if let Some(mesh) = load_mesh(&ctx, area) {
        loaded += 1;
        assert_eq!(mesh.verts.len() % 3, 0);
        assert!(!mesh.verts.is_empty());
      }
    }
    eprintln!("load_mesh: {loaded}/{} areas produced a mesh", areas.len());
  }
}
