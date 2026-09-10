use crate::ctx::Ctx;
use crate::mem::math_utils::read_as_transform;
use crate::structs::prime_structs::GameInstance;

use super::super::WorldRenderer;

impl WorldRenderer {
  /// `CScriptPlatform` complex collision: the `COBBTree` hull(s) off
  /// `x314_treeGroup`, cached model-space by [`Self::sync_obb_hull`] and drawn
  /// into the opaque buffer under the platform's live `transform` (so it tracks
  /// moving platforms for free).
  ///
  /// Returns `true` when the platform has a `COBBTree` hull. A platform built
  /// without a `dcln` has no `treeGroup` — returns `false` so the caller falls
  /// back to the generic
  /// [`draw_physics_actor_collision`](WorldRenderer::draw_physics_actor_collision).
  ///
  /// The per-vertex standability colours are baked in model space by
  /// [`CollisionMesh::build_vertices`][bv]; a platform that only rotates will
  /// have a slightly stale floor/wall/ceiling tint, which is acceptable for an
  /// overlay.
  ///
  /// [bv]: crate::world::collision_mesh::CollisionMesh::build_vertices
  pub(super) fn draw_platform_collision(
    &mut self,
    ctx: &Ctx,
    entity: &GameInstance,
    is_highlighted: bool,
  ) -> bool {
    let Some(transform) = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return false;
    };
    self.sync_obb_hull(ctx, entity, "treeGroup", transform, is_highlighted)
  }
}
