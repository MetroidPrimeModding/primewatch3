//! `WorldRenderer::renderEntities` and the per-class `draw*` functions it
//! dispatches to, plus their small pure helpers (colour ladders, bbox
//! fallback chains, transform math) factored out for unit testing.
//!
//! Split one file per entity class, with the dispatch loop and the shared
//! member-walk / screen-projection helpers here in `mod.rs`.

mod actor;
mod ai;
mod bomb;
mod chozo_ghost;
mod collision_actor;
mod dock;
mod physics_actor;
mod pickup;
mod player;
mod power_bomb;
mod projectile;
mod trigger;

use std::collections::{BTreeMap, HashSet};

use glam::{Mat4, Vec2, Vec3};

use crate::ctx::Ctx;
use crate::mem::game_object_utils::TUniqueID;
use crate::mem::math_utils::read_as_transform;
use crate::mem::math_utils::read_as_vec3;
use crate::structs::prime_structs::GameInstance;

use super::WorldRenderer;

/// Walk a member chain (`entity["a"]["b"]…`), returning `None` on the first
/// missing link — the "`None` -> skip the draw" convention for the per-class
/// draw functions.
pub(crate) fn walk_member(ctx: &Ctx, inst: &GameInstance, path: &[&str]) -> Option<GameInstance> {
  let mut cur = inst.clone();
  for name in path {
    cur = cur.get_member(ctx, name)?;
  }
  Some(cur)
}

/// [`walk_member`] + [`read_as_vec3`] — a `CVector3f` at the end of a member
/// chain.
fn read_vec3_at(ctx: &Ctx, inst: &GameInstance, path: &[&str]) -> Option<Vec3> {
  read_as_vec3(ctx, &walk_member(ctx, inst, path)?)
}

/// `glm::abs(glm::length(min - max)) < 0.1` degeneracy test.
pub(crate) fn is_degenerate_bbox(min: Vec3, max: Vec3) -> bool {
  (min - max).length().abs() < 0.1
}

impl WorldRenderer {
  /// `WorldRenderer::renderEntities` — the active/highlight filter plus the
  /// `extendsClass` dispatch chain. Chain order is load-bearing
  /// (`CCollisionActor` -> `CAi` -> `CPhysicsActor` -> `CActor`): every class
  /// here inherits from the ones below it.
  //
  // `collapsible_if` would suggest folding `if extends_class(X) { if cfg { … } }`
  // into `&&`, but that changes the dispatch — a class match with its config
  // flag off must NOT fall through to a base-class branch.
  #[allow(clippy::collapsible_if)]
  pub(super) fn render_entities(
    &mut self,
    ctx: &Ctx,
    objects: &BTreeMap<TUniqueID, GameInstance>,
    highlighted: &HashSet<u16>,
  ) {
    self.render_buff.set_transform(Mat4::IDENTITY);
    let trigger_flags = trigger::trigger_render_flags(&self.trigger_render_config);

    for entity in objects.values() {
      let active = entity
        .get_member(ctx, "active")
        .and_then(|m| m.read_bool(ctx));
      if active != Some(true) {
        continue;
      }
      let is_highlighted = entity
        .get_member(ctx, "uniqueID")
        .and_then(|m| m.read_u16(ctx))
        .is_some_and(|uid| highlighted.contains(&uid));

      if entity.extends_class(ctx, "CScriptTrigger") {
        let flags = entity
          .get_member(ctx, "triggerFlags")
          .and_then(|m| m.read_u32(ctx))
          .unwrap_or(0);
        if entity.extends_class(ctx, "CScriptWater") {
          if self.trigger_render_config.water {
            self.draw_trigger(ctx, entity, is_highlighted);
          }
        } else if (flags & trigger_flags) != 0 {
          self.draw_trigger(ctx, entity, is_highlighted);
        }
      } else if entity.extends_class(ctx, "CScriptDock") {
        if self.trigger_render_config.docks {
          self.draw_dock(ctx, entity, is_highlighted);
        }
      } else if entity.extends_class(ctx, "CGameProjectile") {
        if self.actor_render_config.render_projectiles {
          self.draw_projectile(ctx, entity, is_highlighted);
        }
      } else if entity.extends_class(ctx, "CBomb") {
        if self.actor_render_config.render_projectiles {
          self.draw_bomb(ctx, entity, is_highlighted);
        }
      } else if entity.extends_class(ctx, "CPowerBomb") {
        if self.actor_render_config.render_projectiles {
          self.draw_power_bomb(ctx, entity, is_highlighted);
        }
      } else if entity.extends_class(ctx, "CPlayer") {
        // player render handled by draw_player
      } else if entity.extends_class(ctx, "CChozoGhost") {
        if self.actor_render_config.render_ai {
          self.draw_chozo_ghost(ctx, entity, is_highlighted, objects);
        }
      } else if entity.extends_class(ctx, "CScriptPickup") {
        if self.actor_render_config.render_pickups {
          self.draw_pickup(ctx, entity, is_highlighted);
        }
      } else if entity.extends_class(ctx, "CCollisionActor") {
        if self.actor_render_config.render_collision_actors {
          self.draw_collision_actor(ctx, entity, is_highlighted);
        }
      } else if entity.extends_class(ctx, "CAi") {
        if self.actor_render_config.render_ai {
          self.draw_ai(ctx, entity, is_highlighted);
        }
      } else if entity.extends_class(ctx, "CPhysicsActor") {
        if self.actor_render_config.render_physics_actors {
          self.draw_physics_actor(ctx, entity, is_highlighted);
        }
      } else if entity.extends_class(ctx, "CActor") {
        if self.actor_render_config.render_actors {
          self.draw_actor(ctx, entity, is_highlighted);
        }
      }
    }
  }

  /// `WorldRenderer::getScreenspacePosForActor`: project the entity's transform
  /// translation to screen pixels, then flip Y for the top-left-origin overlay
  /// space.
  fn screenspace_pos_for_actor(&self, ctx: &Ctx, entity: &GameInstance) -> Option<Vec2> {
    let transform = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))?;
    let pos = transform.w_axis.truncate();
    let s = super::camera::project(pos, self.cam_view, self.cam_projection, self.cam_viewport)?;
    Some(Vec2::new(s.x, self.cam_viewport[3] - s.y))
  }

  /// `WorldRenderer::getScreenspacePosForPhysicsActor`: same as
  /// [`Self::screenspace_pos_for_actor`] but offsets the projected point by the
  /// centre of the actor's bounding box, picked from the `collisionPrimitive` ->
  /// `baseBoundingBox` -> `renderBounds` ladder (the last one is `pos`-relative
  /// asymmetry).
  fn screenspace_pos_for_physics_actor(&self, ctx: &Ctx, entity: &GameInstance) -> Option<Vec2> {
    let transform = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))?;
    let pos = transform.w_axis.truncate();

    let mut min = read_vec3_at(ctx, entity, &["collisionPrimitive", "aabb", "min"])?;
    let mut max = read_vec3_at(ctx, entity, &["collisionPrimitive", "aabb", "max"])?;
    if is_degenerate_bbox(min, max) {
      min = read_vec3_at(ctx, entity, &["baseBoundingBox", "min"])?;
      max = read_vec3_at(ctx, entity, &["baseBoundingBox", "max"])?;
    }
    if is_degenerate_bbox(min, max) {
      min = read_vec3_at(ctx, entity, &["renderBounds", "min"])? - pos;
      max = read_vec3_at(ctx, entity, &["renderBounds", "max"])? - pos;
    }

    let text_pos = (min + max) / 2.0;
    let s = super::camera::project(
      pos + text_pos,
      self.cam_view,
      self.cam_projection,
      self.cam_viewport,
    )?;
    Some(Vec2::new(s.x, self.cam_viewport[3] - s.y))
  }
}
