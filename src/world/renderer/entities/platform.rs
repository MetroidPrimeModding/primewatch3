use glam::Vec4;

use crate::ctx::Ctx;
use crate::gl::shapes;
use crate::structs::prime_structs::GameInstance;
use crate::world::platform_collision::load_platform_collision;

use super::super::WorldRenderer;

impl WorldRenderer {
  /// `CScriptPlatform` complex collision: the `COBBTree` hull(s) off
  /// `x314_treeGroup`, drawn into the opaque buffer under the platform's live
  /// `transform` (so it tracks moving platforms for free).
  ///
  /// Returns `true` when it drew a hull. A platform built without a `dcln` has
  /// no `treeGroup` — returns `false` so the caller falls back to the generic
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
    let Some(pc) = load_platform_collision(ctx, entity) else {
      return false;
    };
    if pc.meshes.is_empty() {
      return false;
    }

    self.render_buff.set_transform(pc.transform);
    for mesh in &pc.meshes {
      self.render_buff.add_tris(&mesh.verts);
      if is_highlighted {
        self.render_buff.add_lines(&shapes::generate_cube_lines(
          mesh.min,
          mesh.max,
          Vec4::new(1.0, 0.0, 0.0, 1.0),
        ));
      }
    }
    true
  }
}
