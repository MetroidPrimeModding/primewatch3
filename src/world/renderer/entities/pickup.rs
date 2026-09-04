use glam::Vec2;

use crate::ctx::Ctx;
use crate::defs::item_types::{EItemType, item_type_to_name};
use crate::structs::prime_structs::GameInstance;

use super::super::WorldRenderer;
use super::super::types::OVERLAY_LINE_HEIGHT;

impl WorldRenderer {
  /// `WorldRenderer::drawPickup`: the `drawPhysicsActor` body plus two label
  /// lines — `"<item> <amount>/<capacity>"` above and `"<curTime>/<lifeTime>"`
  /// below the projected point.
  pub(super) fn draw_pickup(&mut self, ctx: &Ctx, entity: &GameInstance, is_highlighted: bool) {
    self.draw_physics_actor(ctx, entity, is_highlighted);

    let Some(screen) = self.screenspace_pos_for_physics_actor(ctx, entity) else {
      return;
    };
    let Some(item_type) = entity
      .get_member(ctx, "itemType")
      .and_then(|m| m.read_u32(ctx))
      .map(EItemType::from_raw)
    else {
      return;
    };
    let amount = entity
      .get_member(ctx, "amount")
      .and_then(|m| m.read_u32(ctx))
      .unwrap_or(0) as i32;
    let capacity = entity
      .get_member(ctx, "capacity")
      .and_then(|m| m.read_u32(ctx))
      .unwrap_or(0) as i32;
    let life_time = entity
      .get_member(ctx, "lifeTime")
      .and_then(|m| m.read_f32(ctx))
      .unwrap_or(0.0);
    let cur_time = entity
      .get_member(ctx, "curTime")
      .and_then(|m| m.read_f32(ctx))
      .unwrap_or(0.0);

    let line1 = format!("{} {}/{}", item_type_to_name(item_type), amount, capacity);
    let line2 = format!("{cur_time:.1}/{life_time:.1}");
    self.add_text_overlay(Vec2::new(screen.x, screen.y - OVERLAY_LINE_HEIGHT), line1);
    self.add_text_overlay(screen, line2);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn item_type_overlay_text_matches_cpp_format() {
    // Sanity on the string the pickup overlay builds.
    let line1 = format!(
      "{} {}/{}",
      item_type_to_name(EItemType::from_raw(4)),
      5,
      250
    );
    assert_eq!(line1, "Missiles 5/250");
    let line2 = format!("{:.1}/{:.1}", 1.25_f32, 30.0_f32);
    assert_eq!(line2, "1.2/30.0");
  }
}
