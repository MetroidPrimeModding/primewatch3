//! `CCollidableOBBTreeGroup` collision: the walk from an owner entity to the
//! `COBBTree`(s) hanging off a `rstl::single_ptr<CCollidableOBBTreeGroup>`
//! member, and the triangle-soup build. Used for `CScriptPlatform`'s
//! `x314_treeGroup` and for the `CPhysicsActor` subclasses whose
//! `GetCollisionPrimitive` override returns an OBB group
//! (`CPuddleToadGamma::x5e4_collisionTreePrim`).
//!
//! Only entities constructed with a `dcln` argument have this — the pointer is
//! null otherwise (`CScriptPlatform::HasComplexCollision`). The triangles are
//! reconstructed exactly as `COBBTree::GetSurface` does; the edge/vertex walk is
//! the same one [`CollisionMesh::build_vertices`] already runs for the area
//! octree, so the raw arrays are copied into a [`CollisionMesh`] and handed to
//! that.
//!
//! The mesh is **model-space** (the OBB tree stores local coordinates); the
//! caller applies the owner's `transform` (both `CScriptPlatform` and
//! `CPuddleToadGamma` override `GetPrimitiveTransform` to the full actor
//! transform, so the hull rotates with the actor).

use glam::Vec3;

use crate::ctx::Ctx;
use crate::mem::math_utils::read_as_vec3;
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

/// The `CCollidableOBBTreeGroupContainer` address behind `owner.<member>` (an
/// inline `rstl::single_ptr<CCollidableOBBTreeGroup>`), or `None` when the owner
/// has no complex collision (null pointer).
///
/// Cheap — just the pointer chain, no array reads. Callers cache the built hull
/// ([`load_obb_group_meshes`]) on this address: the container and its
/// `COBBTree`s are immutable static data loaded with the owner's `dcln`, so only
/// the owner's live transform needs re-reading each frame.
pub fn obb_group_container_addr(ctx: &Ctx, owner: &GameInstance, member: &str) -> Option<u32> {
  let group = owner.get_member(ctx, member)?.get_member(ctx, "value")?;
  if group.address == 0 {
    return None;
  }
  let container = group.get_member(ctx, "container")?;
  (container.address != 0).then_some(container.address)
}

/// `owner -> <member>.value -> container -> trees[] -> COBBTree`, one
/// model-space [`CollisionMesh`] per tree. `<member>` is an inline
/// `rstl::single_ptr<CCollidableOBBTreeGroup>` — `CScriptPlatform::treeGroup`
/// or a `CPhysicsActor` subclass whose `GetCollisionPrimitive` override returns
/// an OBB tree group (`CPuddleToadGamma::collisionTreePrim`).
///
/// `None` when the pointer is null (no complex collision) or a structural link
/// is unreadable; an empty `Vec` is possible if every tree fails validation.
pub fn load_obb_group_meshes(
  ctx: &Ctx,
  owner: &GameInstance,
  member: &str,
) -> Option<Vec<CollisionMesh>> {
  // `["value"]` auto-derefs the owned pointer. Null => no complex collision.
  let group = owner.get_member(ctx, member)?.get_member(ctx, "value")?;
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
  use crate::mem::math_utils::read_as_transform;
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

  /// `mem1_stone_toad.raw`: Samus standing on a `CPuddleToadGamma`, whose
  /// `GetCollisionPrimitive` override returns a `dcln` OBB group.
  fn load_stone_toad_dump() -> Option<GameMemory> {
    let path = std::env::var("PRIMEWATCH_STONE_TOAD_RAW")
      .unwrap_or_else(|_| format!("{}/mem1_stone_toad.raw", env!("CARGO_MANIFEST_DIR")));
    if !std::path::Path::new(&path).exists() {
      eprintln!("skipping stone_toad dump test: {path} not found");
      return None;
    }
    let mut mem = GameMemory::new();
    mem.load_from_file(&path).expect("read stone toad dump");
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
    assert!(obb_group_container_addr(&ctx, &platform, "treeGroup").is_none());
    assert!(load_obb_group_meshes(&ctx, &platform, "treeGroup").is_none());
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

    let transform = read_as_transform(&ctx, &platform.get_member(&ctx, "transform").unwrap())
      .expect("platform transform");
    let meshes =
      load_obb_group_meshes(&ctx, &platform, "treeGroup").expect("platform has complex collision");
    assert!(!meshes.is_empty(), "expected at least one COBBTree");

    for (i, m) in meshes.iter().enumerate() {
      eprintln!(
        "tree {i}: {} verts, {} tris, model bounds {:?}..{:?}",
        m.raw_verts.len(),
        m.tri_count(),
        m.min,
        m.max
      );
    }

    let areas = get_areas(&ctx);

    for mesh in &meshes {
      assert_eq!(mesh.verts.len() % 3, 0);
      assert!(!mesh.verts.is_empty(), "reconstructed a non-empty tri soup");
      // Every reconstructed index resolved (no silent clamp to vert 0 only).
      assert!(mesh.raw_verts.len() > 3);

      // Model-space verts, transformed by the platform, should land inside some
      // loaded area's AABB — a sanity check on both the offsets and the walk.
      let world_min = transform.transform_point3(mesh.min);
      let world_max = transform.transform_point3(mesh.max);
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

  #[test]
  fn stone_toad_obb_group_walks_via_the_shared_helper() {
    let Some(mem) = load_stone_toad_dump() else {
      return;
    };
    let structs = load_defs();
    let ctx = Ctx::new(&structs, &mem);

    let toad = get_all_objects(&ctx)
      .into_values()
      .find(|e| e.extends_class(&ctx, "CPuddleToadGamma"))
      .expect("a CPuddleToadGamma in the dump (needs its MP1_VTABLES entry to retype)");

    let meshes = load_obb_group_meshes(&ctx, &toad, "collisionTreePrim")
      .expect("stone toad has a dcln OBB group");
    assert!(!meshes.is_empty(), "expected at least one COBBTree");
    for m in &meshes {
      assert_eq!(m.verts.len() % 3, 0);
      assert!(!m.verts.is_empty(), "reconstructed a non-empty tri soup");
      assert!(m.raw_verts.len() > 3);
    }

    // The hull must ride the actor's full transform, not translation only:
    // this toad is visibly rotated, so the rotation basis is far from identity.
    let xf = read_as_transform(&ctx, &toad.get_member(&ctx, "transform").unwrap()).unwrap();
    let rot = glam::Mat3::from_cols(
      xf.x_axis.truncate(),
      xf.y_axis.truncate(),
      xf.z_axis.truncate(),
    );
    assert!(
      (rot - glam::Mat3::IDENTITY)
        .to_cols_array()
        .iter()
        .any(|c| c.abs() > 0.1),
      "expected a rotated toad; got {rot:?}"
    );

    let areas = get_areas(&ctx);
    for mesh in &meshes {
      let center = xf.transform_point3((mesh.min + mesh.max) * 0.5);
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
        "toad collision center {center:?} not in any area AABB"
      );
    }
  }
}
