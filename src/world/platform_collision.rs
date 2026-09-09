//! `CScriptPlatform` complex collision: the walk from a platform entity to the
//! `COBBTree`(s) hanging off its `x314_treeGroup`, and the triangle-soup build.
//!
//! Only platforms constructed with a `dcln` argument have this — `treeGroup` is
//! null otherwise (`CScriptPlatform::HasComplexCollision`). The triangles are
//! reconstructed exactly as `COBBTree::GetSurface` does; the edge/vertex walk is
//! the same one [`CollisionMesh::build_vertices`] already runs for the area
//! octree, so the raw arrays are copied into a [`CollisionMesh`] and handed to
//! that.
//!
//! The mesh is **model-space** (the OBB tree stores local coordinates); the
//! caller applies the platform's `transform`.

use glam::{Mat4, Vec3};

use crate::ctx::Ctx;
use crate::mem::math_utils::{read_as_transform, read_as_vec3};
use crate::structs::prime_structs::GameInstance;
use crate::world::collision_mesh::{CollisionMesh, ECollisionMaterial};

/// `COBBTree::x0_magic` — `verify_deaf_babe` rejects anything else. Doubles as a
/// cheap validation that the whole pointer walk landed on a real tree.
const DEAFBABE: u32 = 0xDEAF_BABE;

/// Defensive caps on the `SIndexData` vector counts (a bad pointer walk lands on
/// garbage lengths). A platform's collision hull is tiny — hundreds of tris at
/// the very most.
const COUNT_CAP: u32 = 20_000;
/// Defensive cap on `CCollidableOBBTreeGroupContainer::x0_trees`.
const TREE_CAP: u32 = 64;

/// One platform's model-space collision, ready to draw under `transform`.
pub struct PlatformCollision {
  /// World transform read off the platform's `CActor::transform`.
  pub transform: Mat4,
  /// Model-space triangle soup, one per `COBBTree` in the container. Each has
  /// [`CollisionMesh::verts`] built; no BVH (OBB platforms aren't ray-traced).
  pub meshes: Vec<CollisionMesh>,
}

/// Full walk for one platform: `transform` + every `COBBTree` mesh. Returns
/// `None` when the platform has no complex collision (null `treeGroup`) or the
/// walk hits an unreadable link; an empty `meshes` is possible if every tree
/// fails validation.
pub fn load_platform_collision(ctx: &Ctx, platform: &GameInstance) -> Option<PlatformCollision> {
  let transform = read_as_transform(ctx, &platform.get_member(ctx, "transform")?)?;
  let meshes = load_platform_meshes(ctx, platform)?;
  Some(PlatformCollision { transform, meshes })
}

/// `platform -> treeGroup.value -> container -> trees[] -> COBBTree`, one
/// model-space [`CollisionMesh`] per tree. `None` when `treeGroup` is null or a
/// structural link is unreadable.
pub fn load_platform_meshes(ctx: &Ctx, platform: &GameInstance) -> Option<Vec<CollisionMesh>> {
  // `treeGroup` is an inline `rstl::single_ptr<CCollidableOBBTreeGroup>`;
  // `["value"]` auto-derefs the pointer. Null => no complex collision.
  let group = platform
    .get_member(ctx, "treeGroup")?
    .get_member(ctx, "value")?;
  if group.address == 0 {
    return None;
  }

  // `container` is `*CCollidableOBBTreeGroupContainer` (auto-deref'd).
  let container = group.get_member(ctx, "container")?;
  if container.address == 0 {
    return None;
  }

  let trees = container.get_member(ctx, "trees")?;
  let count = trees.get_member(ctx, "count")?.read_u32(ctx)?.min(TREE_CAP);
  let first = trees.get_member(ctx, "first")?;

  let mut meshes = Vec::new();
  for i in 0..count {
    // Each element is `rstl::autoptr<COBBTree>` (stride 8); `["value"]`
    // auto-derefs the owned `*COBBTree`.
    let Some(tree) = first.element(ctx, i).get_member(ctx, "value") else {
      continue;
    };
    if tree.address == 0 {
      continue;
    }
    if let Some(mesh) = load_obb_tree(ctx, &tree) {
      meshes.push(mesh);
    }
  }
  Some(meshes)
}

/// One `COBBTree` -> model-space [`CollisionMesh`]. Reads the seven
/// `SIndexData` arrays, reconstructs triangles the way `COBBTree::GetSurface`
/// does (surface `i` uses edge indices `surfaceIndices[3i .. 3i+3]`, material
/// `materials[surfaceMaterials[i]]`), and runs
/// [`CollisionMesh::build_vertices`].
///
/// `None` if the magic word is wrong (bad pointer walk) or a count is
/// out of range.
fn load_obb_tree(ctx: &Ctx, tree: &GameInstance) -> Option<CollisionMesh> {
  if tree.get_member(ctx, "magic")?.read_u32(ctx)? != DEAFBABE {
    return None;
  }

  let idx = tree.get_member(ctx, "indexData")?;

  let materials = read_u32_vec(ctx, &idx.get_member(ctx, "materials")?)?;
  let edges = read_edge_vec(ctx, &idx.get_member(ctx, "edges")?)?;
  let surface_materials = read_u8_vec(ctx, &idx.get_member(ctx, "surfaceMaterials")?)?;
  let surface_indices = read_u16_vec(ctx, &idx.get_member(ctx, "surfaceIndices")?)?;
  let verts = read_vec3_vec(ctx, &idx.get_member(ctx, "vertices")?)?;

  // `surfaceIndices` is 3 edge indices per surface; trust `surfaceMaterials`
  // for the surface count and clamp to what's actually there.
  let surf_count = surface_materials.len().min(surface_indices.len() / 3);

  let mut mesh = CollisionMesh {
    raw_verts: verts,
    raw_edges: edges,
    materials: materials.into_iter().map(ECollisionMaterial).collect(),
    raw_polys: Vec::with_capacity(surf_count),
    raw_poly_materials: Vec::with_capacity(surf_count),
    ..Default::default()
  };
  for i in 0..surf_count {
    mesh.raw_polys.push([
      surface_indices[i * 3],
      surface_indices[i * 3 + 1],
      surface_indices[i * 3 + 2],
    ]);
    mesh.raw_poly_materials.push(surface_materials[i] as u16);
  }

  (mesh.min, mesh.max) = bounds(&mesh.raw_verts);
  mesh.build_vertices();
  Some(mesh)
}

/// Local-space AABB of the vertex list (`(0, 0)` when empty).
fn bounds(verts: &[Vec3]) -> (Vec3, Vec3) {
  verts.iter().fold((Vec3::ZERO, Vec3::ZERO), |(mn, mx), &v| {
    (mn.min(v), mx.max(v))
  })
}

/// `vec["count"]` clamped to [`COUNT_CAP`]; `None` if the count itself is
/// unreadable (structural miss).
fn vec_len(ctx: &Ctx, vec: &GameInstance) -> Option<u32> {
  Some(vec.get_member(ctx, "count")?.read_u32(ctx)?.min(COUNT_CAP))
}

fn read_u32_vec(ctx: &Ctx, vec: &GameInstance) -> Option<Vec<u32>> {
  let n = vec_len(ctx, vec)?;
  let first = vec.get_member(ctx, "first")?;
  Some(
    (0..n)
      .map(|i| first.element(ctx, i).read_u32(ctx).unwrap_or(0))
      .collect(),
  )
}

fn read_u16_vec(ctx: &Ctx, vec: &GameInstance) -> Option<Vec<u16>> {
  let n = vec_len(ctx, vec)?;
  let first = vec.get_member(ctx, "first")?;
  Some(
    (0..n)
      .map(|i| first.element(ctx, i).read_u16(ctx).unwrap_or(0))
      .collect(),
  )
}

fn read_u8_vec(ctx: &Ctx, vec: &GameInstance) -> Option<Vec<u8>> {
  let n = vec_len(ctx, vec)?;
  let first = vec.get_member(ctx, "first")?;
  Some(
    (0..n)
      .map(|i| first.element(ctx, i).read_u8(ctx).unwrap_or(0))
      .collect(),
  )
}

fn read_edge_vec(ctx: &Ctx, vec: &GameInstance) -> Option<Vec<[u16; 2]>> {
  let n = vec_len(ctx, vec)?;
  let first = vec.get_member(ctx, "first")?;
  Some(
    (0..n)
      .map(|i| {
        let e = first.element(ctx, i);
        [
          e.get_member(ctx, "edge1")
            .and_then(|m| m.read_u16(ctx))
            .unwrap_or(0),
          e.get_member(ctx, "edge2")
            .and_then(|m| m.read_u16(ctx))
            .unwrap_or(0),
        ]
      })
      .collect(),
  )
}

fn read_vec3_vec(ctx: &Ctx, vec: &GameInstance) -> Option<Vec<Vec3>> {
  let n = vec_len(ctx, vec)?;
  let first = vec.get_member(ctx, "first")?;
  Some(
    (0..n)
      .map(|i| read_as_vec3(ctx, &first.element(ctx, i)).unwrap_or(Vec3::ZERO))
      .collect(),
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::mem::area_utils::get_areas;
  use crate::mem::game_memory::GameMemory;
  use crate::mem::game_object_utils::get_all_objects;
  use crate::structs::prime_structs::GameStructs;

  fn load_defs() -> GameStructs {
    let mut structs = GameStructs::new_empty();
    structs
      .load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/prime_defs"))
      .expect("load prime_defs");
    structs
  }

  /// The platform-collision dump: editor id `0x0014009B` is a platform with a
  /// `treeGroup`, Samus standing on it.
  fn load_platform_dump() -> Option<GameMemory> {
    let path = std::env::var("PRIMEWATCH_PLATFORM_RAW")
      .unwrap_or_else(|_| format!("{}/mem1_platform_collision.raw", env!("CARGO_MANIFEST_DIR")));
    if !std::path::Path::new(&path).exists() {
      eprintln!("skipping platform_collision dump test: {path} not found");
      return None;
    }
    let mut mem = GameMemory::new();
    mem.load_from_file(&path).expect("read platform dump");
    Some(mem)
  }

  #[test]
  fn bounds_of_empty_is_zero() {
    assert_eq!(bounds(&[]), (Vec3::ZERO, Vec3::ZERO));
  }

  #[test]
  fn bounds_spans_points() {
    let (mn, mx) = bounds(&[Vec3::new(-1.0, 2.0, 0.0), Vec3::new(3.0, -4.0, 5.0)]);
    assert_eq!(mn, Vec3::new(-1.0, -4.0, 0.0));
    assert_eq!(mx, Vec3::new(3.0, 2.0, 5.0));
  }

  #[test]
  fn load_on_zeroed_memory_does_not_panic() {
    let structs = load_defs();
    let mem = GameMemory::new();
    let ctx = Ctx::new(&structs, &mem);
    let platform = GameInstance::new(0x8000_0000, "CScriptPlatform".to_string());
    assert!(load_platform_collision(&ctx, &platform).is_none());
  }

  #[test]
  fn parses_the_known_platform_from_the_dump() {
    let Some(mem) = load_platform_dump() else {
      return;
    };
    let structs = load_defs();
    let ctx = Ctx::new(&structs, &mem);

    let platform = get_all_objects(&ctx)
      .into_values()
      .find(|e| {
        e.extends_class(&ctx, "CScriptPlatform")
          && e
            .get_member(&ctx, "editorID")
            .and_then(|m| m.read_u32(&ctx))
            == Some(0x0014_009B)
      })
      .expect("platform 0x0014009B present in the dump");

    let pc = load_platform_collision(&ctx, &platform).expect("platform has complex collision");
    assert!(!pc.meshes.is_empty(), "expected at least one COBBTree");

    for (i, m) in pc.meshes.iter().enumerate() {
      eprintln!(
        "tree {i}: {} verts, {} tris, model bounds {:?}..{:?}",
        m.raw_verts.len(),
        m.tri_count(),
        m.min,
        m.max
      );
    }

    let areas = get_areas(&ctx);

    for mesh in &pc.meshes {
      assert_eq!(mesh.verts.len() % 3, 0);
      assert!(!mesh.verts.is_empty(), "reconstructed a non-empty tri soup");
      // Every reconstructed index resolved (no silent clamp to vert 0 only).
      assert!(mesh.raw_verts.len() > 3);

      // Model-space verts, transformed by the platform, should land inside some
      // loaded area's AABB — a sanity check on both the offsets and the walk.
      let world_min = pc.transform.transform_point3(mesh.min);
      let world_max = pc.transform.transform_point3(mesh.max);
      let center = (world_min + world_max) * 0.5;
      let in_an_area = areas.iter().any(|a| {
        let mn = read_as_vec3(&ctx, &a.member(&ctx, "aabb").member(&ctx, "min"));
        let mx = read_as_vec3(&ctx, &a.member(&ctx, "aabb").member(&ctx, "max"));
        match (mn, mx) {
          (Some(mn), Some(mx)) => center.cmpge(mn).all() && center.cmple(mx).all(),
          _ => false,
        }
      });
      assert!(
        in_an_area,
        "platform collision center {center:?} not in any area AABB"
      );
    }
  }
}
