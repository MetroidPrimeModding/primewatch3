use crate::ctx::Ctx;
use crate::structs::prime_structs::GameInstance;

use super::super::WorldRenderer;
use super::walk_member;

impl WorldRenderer {
  /// `WorldRenderer::drawAi`: the `drawPhysicsActor` body plus a
  /// `healthInfo.health` label over the actor.
  pub(super) fn draw_ai(&mut self, ctx: &Ctx, entity: &GameInstance, is_highlighted: bool) {
    self.draw_physics_actor(ctx, entity, is_highlighted);

    if let Some(screen) = self.screenspace_pos_for_physics_actor(ctx, entity)
      && let Some(health) =
        walk_member(ctx, entity, &["healthInfo", "health"]).and_then(|m| m.read_f32(ctx))
    {
      self.add_text_overlay(screen, format!("{health:.1}"));
    }
  }
}
