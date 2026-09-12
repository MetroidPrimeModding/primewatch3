use crate::ctx::Ctx;
use crate::structs::prime_structs::GameInstance;
use crate::world::renderer::WorldRenderer;

impl WorldRenderer {
  /// `WorldRenderer::drawCollisionActor` minus the dead `pos`. Axis cross on the
  /// opaque buffer, then the aabb / sphere / obbTreeGroup primitive ladder
  /// (first non-null wins).
  pub(super) fn draw_door(&mut self, ctx: &Ctx, entity: &GameInstance, is_highlighted: bool) {
    if self.actor_render_config.render_physics_collision {
      self.draw_physics_actor_collision(ctx, entity, is_highlighted);
    }
    if self.actor_render_config.render_physics_actors {
      self.draw_physics_actor(ctx, entity, is_highlighted);
    }
  }
}
