use glam::Vec4;

use crate::ctx::Ctx;
use crate::gl::shapes;
use crate::mem::math_utils::read_as_transform;
use crate::structs::prime_structs::GameInstance;

use super::super::WorldRenderer;
use super::read_vec3_at;

impl WorldRenderer {
  /// `WorldRenderer::drawDock`. `min`/`max` are inherited from
  /// `CPhysicsActor::collisionPrimitive`.
  pub(super) fn draw_dock(&mut self, ctx: &Ctx, entity: &GameInstance, is_highlighted: bool) {
    let Some(min) = read_vec3_at(ctx, entity, &["collisionPrimitive", "aabb", "min"]) else {
      return;
    };
    let Some(max) = read_vec3_at(ctx, entity, &["collisionPrimitive", "aabb", "max"]) else {
      return;
    };
    let Some(transform) = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return;
    };
    let color = if is_highlighted {
      Vec4::new(1.0, 0.0, 0.0, 0.5)
    } else {
      Vec4::new(0.5, 1.0, 0.5, 0.5)
    };
    self.translucent_render_buff.set_transform(transform);
    self
      .translucent_render_buff
      .add_tris(&shapes::generate_cube(min, max, color));
  }
}
