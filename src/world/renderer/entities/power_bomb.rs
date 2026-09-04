use glam::{Vec3, Vec4};

use crate::ctx::Ctx;
use crate::gl::shapes;
use crate::mem::math_utils::read_as_transform;
use crate::structs::prime_structs::GameInstance;

use super::super::WorldRenderer;

impl WorldRenderer {
  /// `WorldRenderer::drawPowerBomb`. No highlight branch. `CPowerBomb : CWeapon`.
  pub(super) fn draw_power_bomb(
    &mut self,
    ctx: &Ctx,
    entity: &GameInstance,
    _is_highlighted: bool,
  ) {
    let Some(cur_time) = entity
      .get_member(ctx, "curTime")
      .and_then(|m| m.read_f32(ctx))
    else {
      return;
    };
    if !(1.0..=4.0).contains(&cur_time) {
      return;
    }
    let Some(cur_radius) = entity
      .get_member(ctx, "curRadius")
      .and_then(|m| m.read_f32(ctx))
    else {
      return;
    };
    let Some(transform) = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return;
    };
    let color = Vec4::new(0.8, 0.4, 0.4, 0.4);
    self.translucent_render_buff.set_color(color.to_array());
    self.translucent_render_buff.set_transform(transform);
    self
      .translucent_render_buff
      .add_tris(&shapes::generate_sphere(Vec3::ZERO, cur_radius, color));
  }
}
