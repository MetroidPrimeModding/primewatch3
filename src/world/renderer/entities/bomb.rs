use glam::{Vec3, Vec4};

use crate::ctx::Ctx;
use crate::gl::shapes;
use crate::mem::math_utils::read_as_transform;
use crate::structs::prime_structs::GameInstance;

use super::super::WorldRenderer;

/// `drawBomb`'s fuse-frame gate: `ceil(fuseTimeSeconds * 60) + 1`. The draw is
/// skipped when this is `<= 0`.
pub(crate) fn bomb_fuse_frames(fuse_time: f32) -> i32 {
  (fuse_time * 60.0).ceil() as i32 + 1
}

/// `drawBomb`'s ball-proximity highlight recompute — the passed-in highlight
/// flag is discarded and this predicate decides. `maxDistance` is the
/// hardcoded `1.5` tweak value.
pub(crate) fn bomb_proximity_highlight(player_pos: Vec3, bomb_pos: Vec3) -> bool {
  let pos_to_ball = player_pos + Vec3::new(0.0, 0.0, 0.7) - bomb_pos;
  pos_to_ball.length() < 1.5 && pos_to_ball.z >= -0.7
}

impl WorldRenderer {
  /// `WorldRenderer::drawBomb`. The passed-in `_is_highlighted` is intentionally
  /// ignored — it is recomputed from ball proximity. The fuse-frame count
  /// is queued as a screen-space overlay.
  pub(super) fn draw_bomb(&mut self, ctx: &Ctx, entity: &GameInstance, _is_highlighted: bool) {
    let Some(fuse_time) = entity
      .get_member(ctx, "fuseTime")
      .and_then(|m| m.read_f32(ctx))
    else {
      return;
    };
    if bomb_fuse_frames(fuse_time) <= 0 {
      return;
    }
    let Some(transform) = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return;
    };
    let pos = transform.w_axis.truncate();
    let is_highlighted = bomb_proximity_highlight(self.player.position, pos);

    let color = if is_highlighted {
      Vec4::new(0.8, 0.0, 0.0, 0.8)
    } else {
      Vec4::new(0.7, 0.5, 0.5, 0.5)
    };

    self.translucent_render_buff.set_color(color.to_array());
    self.translucent_render_buff.set_transform(transform);
    // maxDistance (1.5) - 0.7
    self
      .translucent_render_buff
      .add_tris(&shapes::generate_truncated_sphere(
        Vec3::ZERO,
        1.5 - 0.7,
        0.0,
        color,
      ));

    // HP-style fuse-frame count over the bomb.
    if let Some(screen) = self.screenspace_pos_for_actor(ctx, entity) {
      self.add_text_overlay(screen, format!("{}", bomb_fuse_frames(fuse_time)));
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn bomb_fuse_frames_is_ceil_times_60_plus_1() {
    assert_eq!(bomb_fuse_frames(0.0), 1);
    assert_eq!(bomb_fuse_frames(0.5), 31); // ceil(30) + 1
    assert_eq!(bomb_fuse_frames(1.0), 61);
    assert_eq!(bomb_fuse_frames(0.016), 2); // ceil(0.96) + 1
    // a spent bomb -> non-positive -> draw skipped
    assert!(bomb_fuse_frames(-1.0) <= 0);
  }

  #[test]
  fn bomb_proximity_highlight_predicate() {
    // player and bomb coincident: posToBall = (0,0,0.7), len 0.7 < 1.5, z >= -0.7
    assert!(bomb_proximity_highlight(Vec3::ZERO, Vec3::ZERO));
    // bomb far in the xy plane -> out of range
    assert!(!bomb_proximity_highlight(
      Vec3::ZERO,
      Vec3::new(5.0, 0.0, 0.0)
    ));
    // bomb well above the ball -> posToBall.z = 0.7 - 2.0 = -1.3 < -0.7
    assert!(!bomb_proximity_highlight(
      Vec3::ZERO,
      Vec3::new(0.0, 0.0, 2.0)
    ));
    // boundary: bomb at z = 1.4 -> posToBall.z = -0.7 (>= -0.7), len 0.7 < 1.5
    assert!(bomb_proximity_highlight(
      Vec3::ZERO,
      Vec3::new(0.0, 0.0, 1.4)
    ));
    // player offset carries through
    assert!(bomb_proximity_highlight(
      Vec3::new(10.0, 0.0, 0.0),
      Vec3::new(10.0, 0.5, 0.0)
    ));
  }
}
