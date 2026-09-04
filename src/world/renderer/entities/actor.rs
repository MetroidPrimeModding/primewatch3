use glam::{Mat4, Vec3, Vec4};

use crate::ctx::Ctx;
use crate::gl::shapes;
use crate::mem::math_utils::read_as_transform;
use crate::structs::prime_structs::GameInstance;

use super::super::WorldRenderer;
use super::read_vec3_at;

impl WorldRenderer {
  /// `WorldRenderer::drawActor`. A null `*CModelData` (`address == 0`, or an
  /// unreadable pointer) plus not highlighted plus `!render_all_actors` skips
  /// the actor.
  pub(super) fn draw_actor(&mut self, ctx: &Ctx, entity: &GameInstance, is_highlighted: bool) {
    let model_addr = entity.get_member(ctx, "modelData").map_or(0, |m| m.address);
    if model_addr == 0 && !is_highlighted && !self.actor_render_config.render_all_actors {
      return;
    }

    let Some(min) = read_vec3_at(ctx, entity, &["renderBounds", "min"]) else {
      return;
    };
    let Some(max) = read_vec3_at(ctx, entity, &["renderBounds", "max"]) else {
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
