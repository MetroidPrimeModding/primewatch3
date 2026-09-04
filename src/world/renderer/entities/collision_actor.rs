use glam::{Vec3, Vec4};

use crate::ctx::Ctx;
use crate::gl::shapes;
use crate::mem::math_utils::read_as_transform;
use crate::structs::prime_structs::GameInstance;

use super::super::WorldRenderer;
use super::{read_vec3_at, walk_member};

impl WorldRenderer {
  /// `WorldRenderer::drawCollisionActor` minus the dead `pos`. Axis cross on the
  /// opaque buffer, then the aabb / sphere / obbTreeGroup primitive ladder
  /// (first non-null wins).
  pub(super) fn draw_collision_actor(
    &mut self,
    ctx: &Ctx,
    entity: &GameInstance,
    is_highlighted: bool,
  ) {
    let Some(transform) = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return;
    };

    let color = if is_highlighted {
      Vec4::new(1.0, 0.0, 0.0, 0.5)
    } else {
      Vec4::new(1.0, 1.0, 1.0, 0.5)
    };
    let solid_color = color.with_w(1.0);

    let aabb_addr = entity
      .get_member(ctx, "aabbPrimitive")
      .map_or(0, |m| m.address);
    let sphere_addr = entity
      .get_member(ctx, "spherePrimitive")
      .map_or(0, |m| m.address);
    let obb_addr = entity
      .get_member(ctx, "obbTreeGroupPrimitive")
      .map_or(0, |m| m.address);

    // Set colour/transform on both buffers before the ladder so branches
    // that only push tris/lines inherit them.
    self.translucent_render_buff.set_color(color.to_array());
    self.translucent_render_buff.set_transform(transform);
    self.render_buff.set_color(solid_color.to_array());
    self.render_buff.set_transform(transform);

    self
      .render_buff
      .add_line(Vec3::new(-0.2, 0.0, 0.0), Vec3::new(0.2, 0.0, 0.0));
    self
      .render_buff
      .add_line(Vec3::new(0.0, -0.2, 0.0), Vec3::new(0.0, 0.2, 0.0));
    self
      .render_buff
      .add_line(Vec3::new(0.0, 0.0, -0.2), Vec3::new(0.0, 0.0, 0.2));

    if aabb_addr != 0 {
      let Some(min) = read_vec3_at(ctx, entity, &["aabbPrimitive", "aabb", "min"]) else {
        return;
      };
      let Some(max) = read_vec3_at(ctx, entity, &["aabbPrimitive", "aabb", "max"]) else {
        return;
      };
      self
        .translucent_render_buff
        .add_tris(&shapes::generate_cube(min, max, color));
    } else if sphere_addr != 0 {
      let Some(center) = read_vec3_at(ctx, entity, &["spherePrimitive", "sphere", "origin"]) else {
        return;
      };
      let Some(radius) = walk_member(ctx, entity, &["spherePrimitive", "sphere", "radius"])
        .and_then(|m| m.read_f32(ctx))
      else {
        return;
      };
      self
        .translucent_render_buff
        .add_tris(&shapes::generate_sphere(center, radius, color));
    } else if obb_addr != 0 {
      let Some(min) = read_vec3_at(
        ctx,
        entity,
        &["obbTreeGroupPrimitive", "container", "aabb", "min"],
      ) else {
        return;
      };
      let Some(max) = read_vec3_at(
        ctx,
        entity,
        &["obbTreeGroupPrimitive", "container", "aabb", "max"],
      ) else {
        return;
      };
      self
        .render_buff
        .add_lines(&shapes::generate_cube_lines(min, max, color));
    } else {
      eprintln!("Uhoh! unknown collision actor!");
    }
  }
}
