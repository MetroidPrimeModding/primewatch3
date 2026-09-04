use glam::{Mat4, Vec2, Vec3, Vec4};

use crate::gl::shapes;

use super::super::WorldRenderer;
use super::super::types::PlayerGhost;

/// The `drawPlayer` speed-indicator colour ladder: red when the angle between
/// facing and movement exceeds 90° (or is NaN), otherwise a green ramp that
/// flips to cyan past 95%.
pub(crate) fn player_speed_color(angle: f32) -> Vec4 {
  let half_pi = std::f32::consts::FRAC_PI_2;
  if angle.abs() > half_pi || angle.is_nan() {
    return Vec4::new(1.0, 0.0, 0.0, 1.0);
  }
  let percent = angle / half_pi;
  if percent > 0.95 {
    Vec4::new(0.0, 1.0, 1.0, 1.0)
  } else {
    Vec4::new(0.0, percent * 0.5 + 0.5, 0.0, 1.0)
  }
}

impl WorldRenderer {
  /// Selects the opaque or translucent player buffer by `translucent`, sets its
  /// transform, and pushes `verts` — the `buf = color.a < 0.99 ? … : …` pattern
  /// from `drawPlayer`. These buffers are drawn with `bind_group_noclip` so the
  /// near-player bayer cutout in `fs_mesh` leaves the player / ghost models
  /// alone.
  fn player_buf_add_tris(&mut self, translucent: bool, transform: Mat4, verts: &[crate::gl::Vert]) {
    let buf = if translucent {
      &mut self.player_translucent_render_buff
    } else {
      &mut self.player_render_buff
    };
    buf.set_transform(transform);
    buf.add_tris(verts);
  }

  /// `WorldRenderer::drawPlayer`. The collision shape goes to the opaque or
  /// translucent buffer by `color.a`; the speed indicator is always on the
  /// opaque `render_buff`.
  pub(in super::super) fn draw_player(&mut self, ghost: &PlayerGhost, color: Vec4) {
    let translucent = color.w < 0.99;

    if ghost.is_morphed {
      let model = Mat4::from_translation(ghost.position + Vec3::new(0.0, 0.0, 0.7))
        * Mat4::from_quat(ghost.orientation);
      let tris = shapes::generate_sphere(Vec3::ZERO, 0.7, color);
      self.player_buf_add_tris(translucent, model, &tris);
    } else {
      let tris = shapes::generate_cube(Vec3::new(-0.5, -0.5, 0.0), Vec3::new(0.5, 0.5, 2.7), color);
      self.player_buf_add_tris(translucent, Mat4::from_translation(ghost.position), &tris);
    }

    // Speed indicator — always on the opaque `render_buff`.
    let z = if ghost.is_morphed { 0.7 } else { 2.7 / 2.0 };
    self.render_buff.set_transform(Mat4::from_translation(
      ghost.position + Vec3::new(0.0, 0.0, z),
    ));

    let forward3 = (ghost.orientation * Vec3::Y).normalize();
    let forward = Vec2::new(forward3.x, forward3.y);
    let movement3 = ghost.velocity.normalize();
    let movement = Vec2::new(movement3.x, movement3.y);
    let angle = (forward.dot(movement) / (forward.length() * movement.length())).acos();
    let speed_color = player_speed_color(angle);

    self.render_buff.set_color([1.0, 1.0, 1.0, 1.0]);
    self
      .render_buff
      .add_line(Vec3::ZERO, Vec3::new(forward.x, forward.y, 0.0));
    self.render_buff.set_color(speed_color.to_array());
    self.render_buff.add_line(Vec3::ZERO, ghost.velocity * 0.3);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn player_speed_color_ladder() {
    let half_pi = std::f32::consts::FRAC_PI_2;
    // > 90deg -> red
    assert_eq!(
      player_speed_color(half_pi + 0.1),
      Vec4::new(1.0, 0.0, 0.0, 1.0)
    );
    // NaN -> red
    assert_eq!(player_speed_color(f32::NAN), Vec4::new(1.0, 0.0, 0.0, 1.0));
    // aligned -> green base (percent 0)
    assert_eq!(player_speed_color(0.0), Vec4::new(0.0, 0.5, 0.0, 1.0));
    // near 90deg -> cyan (percent > 0.95)
    assert_eq!(
      player_speed_color(half_pi * 0.98),
      Vec4::new(0.0, 1.0, 1.0, 1.0)
    );
  }
}
