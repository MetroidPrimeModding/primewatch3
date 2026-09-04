use std::collections::BTreeMap;

use glam::Mat4;

use crate::ctx::Ctx;
use crate::mem::game_object_utils::TUniqueID;
use crate::mem::math_utils::read_as_transform;
use crate::structs::prime_structs::GameInstance;

use super::super::WorldRenderer;

impl WorldRenderer {
  /// `WorldRenderer::drawChozoGhost` minus the dead `spaceWarpPosition` read and
  /// the commented-out warp line. Draws the `CAi` body then a magenta line to
  /// the ghost's cover point (resolved by slot id `coverPoint & 0x3FF` in the
  /// object map).
  pub(super) fn draw_chozo_ghost(
    &mut self,
    ctx: &Ctx,
    entity: &GameInstance,
    is_highlighted: bool,
    objects: &BTreeMap<TUniqueID, GameInstance>,
  ) {
    self.draw_ai(ctx, entity, is_highlighted);

    let Some(ghost_pos) = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))
      .map(|tf| tf.w_axis.truncate())
    else {
      return;
    };
    let Some(cover_id) = entity
      .get_member(ctx, "coverPoint")
      .and_then(|m| m.read_u16(ctx))
    else {
      return;
    };
    let cover_id = cover_id & 0x3FF;
    if let Some(cover) = objects.get(&cover_id) {
      let Some(cover_pos) = cover
        .get_member(ctx, "transform")
        .and_then(|m| read_as_transform(ctx, &m))
        .map(|tf| tf.w_axis.truncate())
      else {
        return;
      };
      self.translucent_render_buff.set_transform(Mat4::IDENTITY);
      self.translucent_render_buff.set_color([1.0, 0.0, 1.0, 1.0]);
      self.translucent_render_buff.add_line(ghost_pos, cover_pos);
    }
  }
}
