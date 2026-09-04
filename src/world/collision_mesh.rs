//! The memory -> struct parse ([`load_mesh`]) and the triangle-soup build
//! ([`CollisionMesh::build_vertices`]). No GPU code lives here.

use glam::Vec3;

use crate::ctx::Ctx;
use crate::gl::Vert;
use crate::structs::prime_structs::GameInstance;

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
  Some(res)
}

impl CollisionMesh {
  /// Fills [`CollisionMesh::verts`]
  ///
  /// Every lookup is `.get(..).copied().unwrap_or_default()` (or
  /// `unwrap_or(ECollisionMaterial(0))`) so a corrupt index degrades to a
  /// zero/origin value instead of panicking — the repo "OOB -> skip, never
  /// panic" convention.
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
      let i2;
      let other_line;
      if line1[0] == line2[0] {
        i2 = line2[1];
        other_line = line3;
      } else if line1[0] == line2[1] {
        i2 = line2[0];
        other_line = line3;
      } else if line1[0] == line3[0] {
        i2 = line3[1];
        other_line = line2;
      } else {
        i2 = line3[0];
        other_line = line2;
      }

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

      // this is how the game calculates standability
      // (C++ has a redundant `|| n.z > 0.85` on all three arms; preserved verbatim)
      let mut color = [0.2f32, 0.2, 0.2, 1.0];
      if poly_flags.contains(ECollisionMaterial::FLOOR) || n.z > 0.85 {
        color = [0.4, 0.6, 0.4, 1.0];
      } else if poly_flags.contains(ECollisionMaterial::WALL) || n.z > 0.85 {
        color = [0.6, 0.6, 0.6, 1.0];
      } else if poly_flags.contains(ECollisionMaterial::CEILING) || n.z > 0.85 {
        color = [0.8, 0.5, 0.5, 1.0];
      }

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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::mem::area_utils::get_areas;
  use crate::mem::game_memory::GameMemory;
  use crate::structs::prime_structs::GameStructs;

  /// A single triangle: verts (0,0,0) / (1,0,0) / (0,1,0), edges
  /// `[0,1] / [1,2] / [2,0]`, one poly referencing all three edges, one material.
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
