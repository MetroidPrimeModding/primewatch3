use glam::Vec4;

use crate::ctx::Ctx;
use crate::gl::shapes;
use crate::mem::math_utils::read_as_transform;
use crate::structs::prime_structs::GameInstance;

use super::super::WorldRenderer;
use super::read_vec3_at;

/// The `triggerRenderFlags` assembly in `renderEntities` — `detect_projectiles`
/// fans out to all seven projectile bits.
pub(crate) fn trigger_render_flags(c: &super::super::types::TriggerRenderConfig) -> u32 {
  let mut f = 0u32;
  if c.detect_player {
    f |= 0x1;
  }
  if c.detect_ai {
    f |= 0x2;
  }
  if c.detect_projectiles {
    f |= 0x4 | 0x8 | 0x10 | 0x20 | 0x100 | 0x200 | 0x400;
  }
  if c.detect_bombs {
    f |= 0x40;
  }
  if c.detect_power_bombs {
    f |= 0x80;
  }
  if c.kill_on_enter {
    f |= 0x800;
  }
  if c.detect_morphed_player {
    f |= 0x1000;
  }
  if c.use_collision_impulses {
    f |= 0x2000;
  }
  if c.detect_camera {
    f |= 0x4000;
  }
  if c.use_boolean_intersection {
    f |= 0x8000;
  }
  if c.detect_unmorphed_player {
    f |= 0x10000;
  }
  if c.block_environmental_effects {
    f |= 0x20000;
  }
  f
}

/// The `drawTrigger` colour ladder: default white, water tint, highlight red —
/// highlight always wins.
pub(crate) fn trigger_color(is_water: bool, is_highlighted: bool) -> Vec4 {
  let mut color = Vec4::new(1.0, 1.0, 1.0, 0.5);
  if is_water {
    color = Vec4::new(0.5, 0.5, 1.0, 0.5);
  }
  if is_highlighted {
    color = Vec4::new(1.0, 0.0, 0.0, 0.5);
  }
  color
}

impl WorldRenderer {
  /// `WorldRenderer::drawTrigger`.
  pub(super) fn draw_trigger(&mut self, ctx: &Ctx, entity: &GameInstance, is_highlighted: bool) {
    let Some(min) = read_vec3_at(ctx, entity, &["bounds", "min"]) else {
      return;
    };
    let Some(max) = read_vec3_at(ctx, entity, &["bounds", "max"]) else {
      return;
    };
    let Some(transform) = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return;
    };
    let color = trigger_color(entity.extends_class(ctx, "CScriptWater"), is_highlighted);
    self.translucent_render_buff.set_transform(transform);
    self
      .translucent_render_buff
      .add_tris(&shapes::generate_cube(min, max, color));
  }
}

#[cfg(test)]
mod tests {
  use super::super::super::TriggerRenderConfig;
  use super::*;

  #[test]
  fn trigger_render_flags_default_config() {
    // Defaults: detect_player + detect_unmorphed_player -> 0x1 | 0x10000.
    let f = trigger_render_flags(&TriggerRenderConfig::default());
    assert_eq!(f, 0x1 | 0x10000);
  }

  #[test]
  fn trigger_render_flags_projectiles_fan_out() {
    let cfg = TriggerRenderConfig {
      detect_player: false,
      detect_unmorphed_player: false,
      detect_projectiles: true,
      ..TriggerRenderConfig::default()
    };
    assert_eq!(
      trigger_render_flags(&cfg),
      0x4 | 0x8 | 0x10 | 0x20 | 0x100 | 0x200 | 0x400
    );
  }

  #[test]
  fn trigger_render_flags_all_bits() {
    let cfg = TriggerRenderConfig {
      detect_player: true,
      detect_ai: true,
      detect_projectiles: true,
      detect_bombs: true,
      detect_power_bombs: true,
      kill_on_enter: true,
      detect_morphed_player: true,
      use_collision_impulses: true,
      detect_camera: true,
      use_boolean_intersection: true,
      detect_unmorphed_player: true,
      block_environmental_effects: true,
      water: true,
      docks: true,
    };
    assert_eq!(trigger_render_flags(&cfg), 0x3FFFF);
  }

  #[test]
  fn trigger_color_precedence() {
    // default white
    assert_eq!(trigger_color(false, false), Vec4::new(1.0, 1.0, 1.0, 0.5));
    // water tint
    assert_eq!(trigger_color(true, false), Vec4::new(0.5, 0.5, 1.0, 0.5));
    // highlight wins over water
    assert_eq!(trigger_color(true, true), Vec4::new(1.0, 0.0, 0.0, 0.5));
    assert_eq!(trigger_color(false, true), Vec4::new(1.0, 0.0, 0.0, 0.5));
  }
}
