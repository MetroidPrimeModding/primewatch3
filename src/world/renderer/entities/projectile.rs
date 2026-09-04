use glam::{Mat4, Vec3, Vec4};

use crate::ctx::Ctx;
use crate::gl::shapes;
use crate::mem::math_utils::{read_as_transform, read_as_vec3};
use crate::structs::prime_structs::GameInstance;

use super::super::WorldRenderer;

fn read_vec3_member(ctx: &Ctx, parent: &GameInstance, name: &str) -> Option<Vec3> {
  read_as_vec3(ctx, &parent.get_member(ctx, name)?)
}

/// The `drawProjectile`'s nested `CProjectileWeapon` transform chain:
/// `localToWorldXf * (localXf * projOffset + localOffset) + worldOffset`, with
/// each offset extended to a `w = 0` vec4 so the matrix translations only apply
/// via `localToWorldXf` / `localXf` rotation-scale, and `worldOffset` added in
/// world space.
pub(crate) fn projectile_world_pos(
  local_to_world: Mat4,
  local_xf: Mat4,
  proj_offset: Vec3,
  local_offset: Vec3,
  world_offset: Vec3,
) -> Vec3 {
  (local_to_world * (local_xf * proj_offset.extend(0.0) + local_offset.extend(0.0))
    + world_offset.extend(0.0))
  .truncate()
}

/// `drawProjectile`'s velocity transform: `localToWorldXf * localXf *
/// vec4(velocity, 0)`.
pub(crate) fn projectile_world_vel(local_to_world: Mat4, local_xf: Mat4, velocity: Vec3) -> Vec3 {
  (local_to_world * local_xf * velocity.extend(0.0)).truncate()
}

impl WorldRenderer {
  /// `WorldRenderer::drawProjectile`. The `CProjectileWeapon` at `entity["projectile"]` is inline
  /// (not a pointer).
  pub(super) fn draw_projectile(&mut self, ctx: &Ctx, entity: &GameInstance, is_highlighted: bool) {
    let Some(active) = entity
      .get_member(ctx, "projectileActive")
      .and_then(|m| m.read_bool(ctx))
    else {
      return;
    };
    if !active {
      return;
    }
    let Some(projectile) = entity.get_member(ctx, "projectile") else {
      return;
    };
    let Some(local_to_world) = projectile
      .get_member(ctx, "localToWorldXf")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return;
    };
    let Some(local_xf) = projectile
      .get_member(ctx, "localXf")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return;
    };
    let Some(proj_off) = read_vec3_member(ctx, &projectile, "projOffset") else {
      return;
    };
    let Some(local_off) = read_vec3_member(ctx, &projectile, "localOffset") else {
      return;
    };
    let Some(world_off) = read_vec3_member(ctx, &projectile, "worldOffset") else {
      return;
    };
    let Some(scale) = read_vec3_member(ctx, &projectile, "scale") else {
      return;
    };
    let Some(velocity) = read_vec3_member(ctx, &projectile, "velocity") else {
      return;
    };
    let Some(extent) = entity
      .get_member(ctx, "projExtent")
      .and_then(|m| m.read_f32(ctx))
    else {
      return;
    };

    let pos = projectile_world_pos(local_to_world, local_xf, proj_off, local_off, world_off);
    let vel = projectile_world_vel(local_to_world, local_xf, velocity);

    // component-wise (glam `Vec3 * Vec3` is Hadamard, matching `glm::vec3`).
    let size = Vec3::splat(extent) / 2.0 * scale;
    let min = pos - size;
    let max = pos + size;

    let color = if is_highlighted {
      Vec4::new(1.0, 0.0, 0.0, 0.5)
    } else {
      Vec4::new(0.8, 0.4, 0.4, 0.8)
    };

    // min/max are world-space already -> identity transform for the cube.
    self.translucent_render_buff.set_color(color.to_array());
    self.translucent_render_buff.set_transform(Mat4::IDENTITY);
    self
      .translucent_render_buff
      .add_tris(&shapes::generate_cube(min, max, color));

    if is_highlighted {
      self.translucent_render_buff.set_color([0.8, 0.8, 0.8, 0.5]);
      self
        .translucent_render_buff
        .add_line(pos, pos + vel.normalize() * 1000.0);
    }
    self.translucent_render_buff.set_color([1.0, 0.5, 0.5, 1.0]);
    self
      .translucent_render_buff
      .add_line(pos, pos + vel.normalize() * 0.5);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn approx(a: Vec3, b: Vec3) {
    assert!((a - b).length() < 1e-3, "{a:?} != {b:?}");
  }

  #[test]
  fn projectile_world_pos_identity_transforms_sum_offsets() {
    let pos = projectile_world_pos(
      Mat4::IDENTITY,
      Mat4::IDENTITY,
      Vec3::new(1.0, 2.0, 3.0),
      Vec3::new(0.5, 0.0, 0.0),
      Vec3::new(0.0, 0.0, 10.0),
    );
    approx(pos, Vec3::new(1.5, 2.0, 13.0));
  }

  #[test]
  fn projectile_world_pos_world_offset_is_added_after_local_to_world() {
    let ltw = Mat4::from_translation(Vec3::new(100.0, 0.0, 0.0));
    // proj/local offsets are w=0 -> localToWorldXf translation still applies to
    // the (0,0,0) point via its 4th column since the accumulated vec4 has w=1
    // only from... actually offsets stay w=0, so translation does NOT apply.
    approx(
      projectile_world_pos(ltw, Mat4::IDENTITY, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO),
      Vec3::ZERO,
    );
    // worldOffset is a plain world-space add.
    approx(
      projectile_world_pos(
        ltw,
        Mat4::IDENTITY,
        Vec3::ZERO,
        Vec3::ZERO,
        Vec3::new(0.0, 5.0, 0.0),
      ),
      Vec3::new(0.0, 5.0, 0.0),
    );
  }

  #[test]
  fn projectile_world_vel_rotates_without_translating() {
    let ltw = Mat4::from_translation(Vec3::new(100.0, 0.0, 0.0));
    approx(
      projectile_world_vel(ltw, Mat4::IDENTITY, Vec3::new(0.0, 0.0, 1.0)),
      Vec3::new(0.0, 0.0, 1.0),
    );
    let rot = Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2);
    approx(
      projectile_world_vel(rot, Mat4::IDENTITY, Vec3::new(1.0, 0.0, 0.0)),
      Vec3::new(0.0, 1.0, 0.0),
    );
  }
}
