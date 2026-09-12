use glam::{Mat4, Vec3, Vec4};

use crate::ctx::Ctx;
use crate::gl::shapes;
use crate::mem::math_utils::read_as_transform;
use crate::structs::prime_structs::GameInstance;
use crate::world::collision_mesh::{CMaterialList, collision_box_verts, collision_sphere_verts};
use crate::world::platform_collision::{load_obb_group_meshes, obb_group_container_addr};

use super::super::{ObbHullInstance, WorldRenderer};
use super::{is_degenerate_bbox, read_vec3_at, walk_member};

/// A handful of `CPhysicsActor` subclasses override `GetCollisionPrimitive` to
/// return a member primitive rather than the base `x1c0` `CAABBPrimitive`. Each
/// entry is `(class name, member name, shape)`; the AI collision pass walks this
/// most-derived class first.
const OVERRIDDEN_COLLISION_PRIMITIVES: &[(&str, &str, PrimShape)] = &[
  ("CWarWasp", "collisionSphere", PrimShape::Sphere),
  ("CBabygoth", "collisionAABox", PrimShape::AABox),
  ("CIceSheegoth", "collisionAABox", PrimShape::AABox),
  ("CPuddleSpore", "collisionAABox", PrimShape::AABox),
  ("CMetroid", "collisionSphere", PrimShape::Sphere),
  // `CElitePirate` also covers `COmegaPirate` (inherits `x738_collisionAabb`).
  ("CElitePirate", "collisionAABox", PrimShape::AABox),
  // `CDrone` also has an `x834_28` fall-back-to-base flag, but the decomp never
  // sets it, so the sphere is always effective.
  ("CDrone", "collisionSphere", PrimShape::Sphere),
  // `dcln`-backed OBB tree group (same walk as `CScriptPlatform::treeGroup`).
  ("CPuddleToadGamma", "collisionTreePrim", PrimShape::Obb),
  // Base class — keep last; `CParasite` / `CSeedling` inherit it unchanged.
  ("CWallWalker", "collisionSphere", PrimShape::Sphere),
];

#[derive(Clone, Copy, PartialEq)]
enum PrimShape {
  AABox,
  Sphere,
  Obb,
}

/// The `drawPhysicsActor` bounding-box fallback chain: `collisionPrimitive`
/// aabb (`pos`-offset) -> `baseBoundingBox` (`pos`-offset) -> `renderBounds`
/// (**no** `pos` offset).
pub(crate) fn physics_actor_bbox(
  pos: Vec3,
  collision_primitive: (Vec3, Vec3),
  base_bounding_box: (Vec3, Vec3),
  render_bounds: (Vec3, Vec3),
) -> (Vec3, Vec3) {
  let (mut min, mut max) = (pos + collision_primitive.0, pos + collision_primitive.1);
  if is_degenerate_bbox(min, max) {
    min = pos + base_bounding_box.0;
    max = pos + base_bounding_box.1;
  }
  if is_degenerate_bbox(min, max) {
    min = render_bounds.0;
    max = render_bounds.1;
  }
  (min, max)
}

impl WorldRenderer {
  /// The actor's collision primitive (`x1c0_collisionPrimitive`, a
  /// `CCollidableAABox`) drawn as opaque, standability-tinted, wireframed
  /// geometry — the same look as the area collision mesh — so physics actors
  /// read as solid collision rather than a translucent debug box.
  ///
  /// World box = the local primitive AABB shifted by `translation +
  /// primitiveOffset`; `primitiveOffset` (`CPhysicsActor::GetPrimitiveTransform`)
  /// is `Zero` for most actors. Draws nothing when the primitive is degenerate
  /// (the actor has no real collision hull).
  ///
  /// Subclasses with a richer representation (e.g. `CScriptPlatform`'s
  /// `COBBTree`) draw that instead and skip this.
  pub(super) fn draw_physics_actor_collision(
    &mut self,
    ctx: &Ctx,
    entity: &GameInstance,
    is_highlighted: bool,
  ) {
    // let has_solid = entity
    //   .get_member(ctx, "collisionPrimitive")
    //   .and_then(|p| p.get_member(ctx, "material"))
    //   .and_then(|m| m.read_u64(ctx))
    //   .is_some_and(|mask| CMaterialList(mask).contains(CMaterialList::SOLID));

    let actor_is_solid = entity
      .get_member(ctx, "material")
      .and_then(|m| m.read_u64(ctx))
      .is_some_and(|mask| CMaterialList(mask).contains(CMaterialList::SOLID));

    if !actor_is_solid {
      return;
    }

    let Some(transform) = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return;
    };
    let pos = transform.w_axis.truncate();
    let offset = read_vec3_at(ctx, entity, &["primitiveOffset"]).unwrap_or(Vec3::ZERO);

    let Some(cp_min) = read_vec3_at(ctx, entity, &["collisionPrimitive", "aabb", "min"]) else {
      return;
    };
    let Some(cp_max) = read_vec3_at(ctx, entity, &["collisionPrimitive", "aabb", "max"]) else {
      return;
    };
    self.draw_collision_aabox(pos + offset + cp_min, pos + offset + cp_max, is_highlighted);
  }

  /// The overridden `GetCollisionPrimitive` primitive for the
  /// [`OVERRIDDEN_COLLISION_PRIMITIVES`] classes (`CWarWasp`, `CBabygoth`,
  /// `CPuddleToadGamma`, …), drawn in the same solid, standability-tinted style
  /// as [`Self::draw_physics_actor_collision`].
  ///
  /// Placement follows each class's `GetPrimitiveTransform`: the box / sphere
  /// classes inherit `CPhysicsActor`'s (translation + `primitiveOffset`, no
  /// rotation), but `CPuddleToadGamma` overrides it to the full actor transform
  /// (like `CScriptPlatform`), so its OBB hull rotates with the actor.
  pub(super) fn draw_ai_collision(
    &mut self,
    ctx: &Ctx,
    entity: &GameInstance,
    is_highlighted: bool,
  ) {
    let Some(&(_, member, shape)) = OVERRIDDEN_COLLISION_PRIMITIVES
      .iter()
      .find(|(class, ..)| entity.extends_class(ctx, class))
    else {
      return;
    };

    // Same `kMT_Solid` gate as `draw_physics_actor_collision` — a patterned
    // enemy drops the flag when dead / frozen and then isn't solid collision.
    // The OBB group carries its material one pointer deeper; skip the gate for
    // it, as `draw_platform_collision` does for the identical `treeGroup`.
    if shape != PrimShape::Obb {
      const MT_SOLID: u32 = 19;
      let has_solid = walk_member(ctx, entity, &[member, "material"])
        .and_then(|m| m.read_u64(ctx))
        .is_some_and(|mask| mask & (1u64 << MT_SOLID) != 0);
      if !has_solid {
        return;
      }
    }

    let Some(transform) = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return;
    };
    let base = transform.w_axis.truncate()
      + read_vec3_at(ctx, entity, &["primitiveOffset"]).unwrap_or(Vec3::ZERO);

    match shape {
      PrimShape::AABox => {
        let Some(min) = read_vec3_at(ctx, entity, &[member, "aabb", "min"]) else {
          return;
        };
        let Some(max) = read_vec3_at(ctx, entity, &[member, "aabb", "max"]) else {
          return;
        };
        self.draw_collision_aabox(base + min, base + max, is_highlighted);
      }
      PrimShape::Sphere => {
        let Some(center) = read_vec3_at(ctx, entity, &[member, "sphere", "origin"]) else {
          return;
        };
        let Some(radius) =
          walk_member(ctx, entity, &[member, "sphere", "radius"]).and_then(|m| m.read_f32(ctx))
        else {
          return;
        };
        self.draw_collision_sphere(base + center, radius, is_highlighted);
      }
      PrimShape::Obb => {
        // `CPuddleToadGamma::GetPrimitiveTransform` is the full actor transform
        // plus `primitiveOffset` — the model-space hull rotates with the actor.
        let mut xf = transform;
        xf.w_axis = base.extend(1.0);
        self.sync_obb_hull(ctx, entity, member, xf, is_highlighted);
      }
    }
  }

  /// Ensure the OBB hull geometry behind `owner.<member>` is cached
  /// (`obb_mesh_cache`, keyed by container address — built once, the container
  /// is immutable static data and may be shared by other owners), record/refresh
  /// this owner's pose in `obb_instances` (keyed by `(owner address, member)` —
  /// see [`ObbHullInstance`] for why the container address alone can't identify
  /// an instance), and mark the instance live for this frame's eviction pass.
  ///
  /// The hull tris are drawn from the world-space GPU cache in
  /// [`WorldRenderer::render`]; the red bounds box for a highlighted entity is
  /// added to the immediate buffer here (it tracks the live transform). Returns
  /// whether a hull is present — the caller falls back to a simple primitive
  /// when it isn't.
  pub(super) fn sync_obb_hull(
    &mut self,
    ctx: &Ctx,
    owner: &GameInstance,
    member: &'static str,
    transform: Mat4,
    is_highlighted: bool,
  ) -> bool {
    let Some(container) = obb_group_container_addr(ctx, owner, member) else {
      return false;
    };
    let instance_key = (owner.address, member);
    self.obb_hulls_seen.insert(instance_key);

    let meshes = match self.obb_mesh_cache.entry(container) {
      std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
      std::collections::hash_map::Entry::Vacant(e) => {
        let meshes = load_obb_group_meshes(ctx, owner, member).unwrap_or_default();
        if meshes.is_empty() {
          return false;
        }
        e.insert(meshes)
      }
    };

    if is_highlighted {
      let bounds: Vec<(Vec3, Vec3)> = meshes.iter().map(|m| (m.min, m.max)).collect();
      self.render_buff.set_transform(transform);
      for (min, max) in bounds {
        self.render_buff.add_lines(&shapes::generate_cube_lines(
          min,
          max,
          Vec4::new(1.0, 0.0, 0.0, 1.0),
        ));
      }
    }

    self.obb_instances.insert(
      instance_key,
      ObbHullInstance {
        container,
        transform,
      },
    );
    true
  }

  /// Solid, standability-tinted AABox (the shared tail of
  /// [`Self::draw_physics_actor_collision`] and the box arm of
  /// [`Self::draw_ai_collision`]). No-ops on a degenerate box.
  fn draw_collision_aabox(&mut self, min: Vec3, max: Vec3, is_highlighted: bool) {
    if is_degenerate_bbox(min, max) {
      return;
    }
    self.render_buff.set_transform(Mat4::IDENTITY);
    self.render_buff.add_tris(&collision_box_verts(min, max));
    if is_highlighted {
      self.render_buff.add_lines(&shapes::generate_cube_lines(
        min,
        max,
        Vec4::new(1.0, 0.0, 0.0, 1.0),
      ));
    }
  }

  /// [`Self::draw_collision_aabox`] for a sphere primitive. The highlight is a
  /// red wireframe cube on the sphere's bounds (there is no sphere-line shape),
  /// matching the OBB highlight in `draw_collision_actor`.
  fn draw_collision_sphere(&mut self, center: Vec3, radius: f32, is_highlighted: bool) {
    if radius < 0.05 {
      return;
    }
    self.render_buff.set_transform(Mat4::IDENTITY);
    self
      .render_buff
      .add_tris(&collision_sphere_verts(center, radius));
    if is_highlighted {
      self.render_buff.add_lines(&shapes::generate_cube_lines(
        center - Vec3::splat(radius),
        center + Vec3::splat(radius),
        Vec4::new(1.0, 0.0, 0.0, 1.0),
      ));
    }
  }

  /// `WorldRenderer::drawPhysicsActor`.
  pub(super) fn draw_physics_actor(
    &mut self,
    ctx: &Ctx,
    entity: &GameInstance,
    is_highlighted: bool,
  ) {
    let Some(transform) = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return;
    };
    let pos = transform.w_axis.truncate();

    let Some(cp_min) = read_vec3_at(ctx, entity, &["collisionPrimitive", "aabb", "min"]) else {
      return;
    };
    let Some(cp_max) = read_vec3_at(ctx, entity, &["collisionPrimitive", "aabb", "max"]) else {
      return;
    };
    let Some(bb_min) = read_vec3_at(ctx, entity, &["baseBoundingBox", "min"]) else {
      return;
    };
    let Some(bb_max) = read_vec3_at(ctx, entity, &["baseBoundingBox", "max"]) else {
      return;
    };
    let Some(rb_min) = read_vec3_at(ctx, entity, &["renderBounds", "min"]) else {
      return;
    };
    let Some(rb_max) = read_vec3_at(ctx, entity, &["renderBounds", "max"]) else {
      return;
    };

    let (min, max) = physics_actor_bbox(pos, (cp_min, cp_max), (bb_min, bb_max), (rb_min, rb_max));

    let color = if is_highlighted {
      Vec4::new(1.0, 0.0, 0.0, 0.5)
    } else {
      Vec4::new(1.0, 1.0, 1.0, 0.5)
    };

    self.translucent_render_buff.set_color(color.to_array());
    self.translucent_render_buff.set_transform(Mat4::IDENTITY);
    self
      .translucent_render_buff
      .add_tris(&shapes::generate_cube(min, max, color));

    self.translucent_render_buff.set_transform(transform);
    self
      .translucent_render_buff
      .add_line(Vec3::new(0.0, -0.5, 0.0), Vec3::new(0.0, 0.5, 0.0));
    self
      .translucent_render_buff
      .add_line(Vec3::new(-0.5, 0.0, 0.0), Vec3::new(0.5, 0.0, 0.0));
    self
      .translucent_render_buff
      .add_line(Vec3::new(0.0, 0.0, -0.5), Vec3::new(0.0, 0.0, 0.5));
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn approx(a: Vec3, b: Vec3) {
    assert!((a - b).length() < 1e-3, "{a:?} != {b:?}");
  }

  #[test]
  fn physics_actor_bbox_uses_collision_primitive_when_non_degenerate() {
    let pos = Vec3::new(10.0, 0.0, 0.0);
    let cp = (Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
    let bb = (Vec3::splat(-5.0), Vec3::splat(5.0));
    let rb = (Vec3::splat(-9.0), Vec3::splat(9.0));
    let (min, max) = physics_actor_bbox(pos, cp, bb, rb);
    approx(min, pos + cp.0);
    approx(max, pos + cp.1);
  }

  #[test]
  fn physics_actor_bbox_falls_back_to_base_then_render_bounds() {
    let pos = Vec3::new(10.0, 2.0, 3.0);
    let degen = (Vec3::ZERO, Vec3::ZERO);
    // collisionPrimitive degenerate -> baseBoundingBox (pos-offset).
    let bb = (Vec3::splat(-2.0), Vec3::splat(2.0));
    let (min, max) = physics_actor_bbox(pos, degen, bb, degen);
    approx(min, pos + bb.0);
    approx(max, pos + bb.1);

    // both degenerate -> renderBounds, NOT pos-offset.
    let rb = (Vec3::new(-4.0, -4.0, -4.0), Vec3::new(4.0, 4.0, 4.0));
    let (min, max) = physics_actor_bbox(pos, degen, degen, rb);
    approx(min, rb.0);
    approx(max, rb.1);
  }
}
