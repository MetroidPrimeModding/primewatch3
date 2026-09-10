use glam::{Mat4, Vec3, Vec4};

use crate::ctx::Ctx;
use crate::gl::shapes;
use crate::mem::math_utils::read_as_transform;
use crate::structs::prime_structs::GameInstance;
use crate::world::collision_mesh::collision_box_verts;

use super::super::WorldRenderer;
use super::{is_degenerate_bbox, read_vec3_at};

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
    // Only draw a primitive the game treats as solid collision — matches
    // `CCollisionPrimitive::GetMaterial().HasMaterial(kMT_Solid)`. `material`
    // is the `CMaterialList` u64 mask; `kMT_Solid` is bit 19.
    const MT_SOLID: u32 = 19;
    let has_solid = entity
      .get_member(ctx, "collisionPrimitive")
      .and_then(|p| p.get_member(ctx, "material"))
      .and_then(|m| m.read_u64(ctx))
      .is_some_and(|mask| mask & (1u64 << MT_SOLID) != 0);
    if !has_solid {
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
    let (min, max) = (pos + offset + cp_min, pos + offset + cp_max);
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
