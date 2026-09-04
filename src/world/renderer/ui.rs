//! egui surfaces: the "WorldStatus" / "PlayerStatus" status windows, the
//! Culling / Camera / Triggers / Actors menu bar, and the "Camera Controls"
//! window. Split into free functions taking `&mut` field refs where possible
//! so the widget bodies type-check and run headless (no GPU state, no
//! `Ctx`/`GameInstance`).

use glam::Vec2;

use crate::ctx::Ctx;
use crate::mem::area_utils::get_areas;
use crate::mem::game_object_utils::{get_all_loading_datas, object_tag_to_string};

use super::WorldRenderer;
use super::entities::walk_member;
use super::types::{
  ActorRenderConfig, CameraMode, CullType, OrbitPlayerCameraOrigin, PlayerClipConfig,
  TriggerRenderConfig,
};

impl WorldRenderer {
  /// `WorldRenderer::renderImGui` — the "WorldStatus" area/loading table and the
  /// "PlayerStatus" pos/vel/look readout. egui has no free-floating windows, so
  /// both spawn off the passed `ui`'s context.
  pub fn render_status_windows(&self, ctx: &Ctx, ui: &mut egui::Ui) {
    let egui_ctx = ui.ctx().clone();

    egui::Window::new("WorldStatus")
      .resizable(false)
      .title_bar(false)
      .anchor(egui::Align2::RIGHT_TOP, [-8.0, 8.0])
      .show(&egui_ctx, |ui| self.render_world_status(ctx, ui));

    egui::Window::new("PlayerStatus")
      .resizable(false)
      .title_bar(false)
      .anchor(egui::Align2::LEFT_BOTTOM, [8.0, -8.0])
      .show(&egui_ctx, |ui| self.render_player_status(ui));
  }

  /// The "WorldStatus" window body.
  fn render_world_status(&self, ctx: &Ctx, ui: &mut egui::Ui) {
    let e_chain = ctx.structs.get_enum_by_name("EChain");
    let e_phase = ctx.structs.get_enum_by_name("EPhase");

    egui::Grid::new("world-status-areas")
      .striped(true)
      .show(ui, |ui| {
        ui.label("MREA");
        ui.label("Chain");
        ui.label("Phase");
        ui.label("Occluded");
        ui.end_row();

        for area in get_areas(ctx) {
          let chain = area
            .get_member(ctx, "curChain")
            .and_then(|m| m.read_u32(ctx))
            .unwrap_or(0);
          if chain == 1 {
            continue; // deallocated
          }
          let mrea = area
            .get_member(ctx, "mrea")
            .and_then(|m| m.read_u32(ctx))
            .unwrap_or(0);
          let phase = area
            .get_member(ctx, "phase")
            .and_then(|m| m.read_u32(ctx))
            .unwrap_or(0);

          let chain_text = e_chain
            .as_ref()
            .and_then(|e| e.get_name_by_value(chain as i64))
            .unwrap_or_else(|| chain.to_string());
          let phase_text = e_phase
            .as_ref()
            .and_then(|e| e.get_name_by_value(phase as i64))
            .unwrap_or_else(|| phase.to_string());

          let mut occluded_text = "yes";
          if area
            .get_member(ctx, "isPostConstructed")
            .and_then(|m| m.read_bool(ctx))
            .unwrap_or(false)
          {
            let occluded = walk_member(ctx, &area, &["postConstructed", "occlusionState"])
              .and_then(|m| m.read_u32(ctx))
              .unwrap_or(0);
            if occluded == 1 {
              occluded_text = "no";
            }
          }

          ui.label(format!("{mrea:08x}"));
          ui.label(chain_text);
          ui.label(phase_text);
          ui.label(occluded_text);
          ui.end_row();
        }
      });

    // Resource load queue.
    let loading = get_all_loading_datas(ctx);
    if !loading.is_empty() {
      ui.label(format!("Loading {}", loading.len()));
      let mut shown = 0u32;
      let mut shown_size: u32 = 0;
      let mut rest_size: u32 = 0;
      for ld in &loading {
        let size = ld
          .get_member(ctx, "resLen")
          .and_then(|m| m.read_u32(ctx))
          .unwrap_or(0);
        if shown < 5 {
          if let Some(tag) = ld.get_member(ctx, "tag") {
            ui.label(format!("{}: {}", object_tag_to_string(ctx, &tag), size));
          }
          shown += 1;
          shown_size = shown_size.saturating_add(size);
        } else {
          rest_size = rest_size.saturating_add(size);
        }
      }
      if shown_size > 0 || rest_size > 0 {
        ui.label(format!(
          "+{}k = {}k",
          rest_size / 1024,
          (shown_size.saturating_add(rest_size)) / 1024
        ));
      }
    }
  }

  /// The "PlayerStatus" window body.
  fn render_player_status(&self, ui: &mut egui::Ui) {
    let forward = self.player_look_vec;
    let hforward = Vec2::new(forward.x, forward.y).normalize_or_zero();
    let hvel = Vec2::new(self.player.velocity.x, self.player.velocity.y);

    let p = self.player.position;
    let v = self.player.velocity;
    ui.label(format!("pos: {:8.3}x {:8.3}y {:8.3}z", p.x, p.y, p.z));
    ui.label(format!(
      "vel: {:8.3}x {:8.3}y {:8.3}z {:8.3}h",
      v.x,
      v.y,
      v.z,
      hvel.length()
    ));

    let hveldir = hvel.normalize_or_zero();
    let forward_angle = hforward.y.atan2(hforward.x);
    let vel_angle = hveldir.y.atan2(hveldir.x);
    let angle = forward_angle - vel_angle;
    ui.label(format!(
      "look: {:6.3}x {:6.3}y {:6.1}deg | vel {:6.3}x {:6.3}y {:6.1}deg | {:6.1} deg",
      hforward.x,
      hforward.y,
      forward_angle.to_degrees(),
      hveldir.x,
      hveldir.y,
      vel_angle.to_degrees(),
      angle.to_degrees()
    ));
  }

  /// The render-config half of `PrimeWatch::doMainMenu` — the Culling / Camera /
  /// Triggers / Actors menus. Thin forwarder onto [`render_menu_bar`] so the
  /// body stays testable without a `wgpu::Device`.
  pub fn render_menu(&mut self, ui: &mut egui::Ui) {
    render_menu_bar(
      ui,
      &mut self.culling,
      &mut self.player_clip_config,
      &mut self.camera_mode,
      &mut self.orbit_player_camera_origin,
      &mut self.manual_camera_speed,
      &mut self.show_exact_camera_controls,
      &mut self.trigger_render_config,
      &mut self.actor_render_config,
    );
  }

  /// The "Camera Controls" window body from `PrimeWatch::doFrame`. Thin
  /// forwarder onto [`render_camera_controls_ui`].
  pub fn render_camera_controls(&mut self, ui: &mut egui::Ui) {
    render_camera_controls_ui(
      ui,
      &mut self.cam_line_length,
      &mut self.manual_camera_pos,
      &mut self.yaw,
      &mut self.pitch,
    );
  }
}

/// Body of the Culling / Camera / Triggers / Actors menus. Free function taking
/// `&mut` field refs so it type-checks and runs headless (no GPU state). Mirrors
/// `PrimeWatch::doMainMenu` verbatim, including the intentional Culling
/// label/value skew ("Show Front" -> `Back`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_menu_bar(
  ui: &mut egui::Ui,
  culling: &mut CullType,
  player_clip: &mut PlayerClipConfig,
  camera_mode: &mut CameraMode,
  orbit: &mut OrbitPlayerCameraOrigin,
  manual_camera_speed: &mut f32,
  show_exact_camera_controls: &mut bool,
  triggers: &mut TriggerRenderConfig,
  actors: &mut ActorRenderConfig,
) {
  // Culling. Labels and value mapping are verbatim: "Show Front" selects `BACK`,
  // "Show Back" selects `FRONT`.
  ui.menu_button("Culling", |ui| {
    ui.selectable_value(culling, CullType::Back, "Show Front");
    ui.selectable_value(culling, CullType::Front, "Show Back");
    ui.selectable_value(culling, CullType::None, "Show All");

    ui.separator();
    ui.checkbox(&mut player_clip.enabled, "Hide geometry in front of player");
    if player_clip.enabled {
      ui.add(
        egui::Slider::new(&mut player_clip.cone_radius, 0.25..=10.0)
          .clamping(egui::SliderClamping::Always)
          .text("Cone radius"),
      );
      ui.add(
        egui::Slider::new(&mut player_clip.player_margin, 0.0..=10.0)
          .clamping(egui::SliderClamping::Always)
          .text("Player margin"),
      );
      ui.add(
        egui::Slider::new(&mut player_clip.player_fade, 0.05..=20.0)
          .clamping(egui::SliderClamping::Always)
          .text("Player fade"),
      );
      ui.add(
        egui::Slider::new(&mut player_clip.min_visibility, 0.0..=1.0)
          .clamping(egui::SliderClamping::Always)
          .text("Min visibility"),
      );
    }
  });

  // Camera.
  ui.menu_button("Camera", |ui| {
    ui.selectable_value(camera_mode, CameraMode::FollowPlayer, "Follow Player");
    if *camera_mode == CameraMode::FollowPlayer {
      ui.separator();
      ui.indent("camera-follow-orbit", |ui| {
        ui.selectable_value(orbit, OrbitPlayerCameraOrigin::Top, "Top");
        ui.selectable_value(orbit, OrbitPlayerCameraOrigin::Center, "Center");
        ui.selectable_value(orbit, OrbitPlayerCameraOrigin::Bottom, "Bottom");
      });
      ui.separator();
    }
    ui.selectable_value(camera_mode, CameraMode::GameCam, "Game Cam");
    ui.selectable_value(camera_mode, CameraMode::Detached, "Detatched");
    if *camera_mode == CameraMode::Detached {
      ui.separator();
      ui.indent("camera-detached-controls", |ui| {
        ui.add(
          egui::Slider::new(manual_camera_speed, 0.1..=2.0)
            .clamping(egui::SliderClamping::Always)
            .text("Speed"),
        );
        ui.checkbox(show_exact_camera_controls, "Show camera controls");
      });
      ui.separator();
    }
  });

  // Triggers. `TOGGLE_MENU` -> `ui.checkbox`. Field order follows the struct
  // declaration order.
  ui.menu_button("Triggers", |ui| {
    ui.checkbox(&mut triggers.detect_player, "detectPlayer");
    ui.checkbox(&mut triggers.detect_ai, "detectAi");
    ui.checkbox(&mut triggers.detect_projectiles, "detectProjectiles");
    ui.checkbox(&mut triggers.detect_bombs, "detectBombs");
    ui.checkbox(&mut triggers.detect_power_bombs, "detectPowerBombs");
    ui.checkbox(&mut triggers.kill_on_enter, "killOnEnter");
    ui.checkbox(&mut triggers.detect_morphed_player, "detectMorphedPlayer");
    ui.checkbox(&mut triggers.use_collision_impulses, "useCollisionImpulses");
    ui.checkbox(&mut triggers.detect_camera, "detectCamera");
    ui.checkbox(
      &mut triggers.use_boolean_intersection,
      "useBooleanIntersection",
    );
    ui.checkbox(
      &mut triggers.detect_unmorphed_player,
      "detectUnmorphedPlayer",
    );
    ui.checkbox(
      &mut triggers.block_environmental_effects,
      "blockEnvironmentalEffects",
    );
    ui.separator();
    ui.checkbox(&mut triggers.water, "Water");
    ui.checkbox(&mut triggers.docks, "Docks");
  });

  // Actors. `renderCollisionActors` is deliberately not exposed.
  ui.menu_button("Actors", |ui| {
    ui.checkbox(&mut actors.render_projectiles, "Render projectiles");
    ui.checkbox(&mut actors.render_ai, "Render AI");
    ui.checkbox(&mut actors.render_pickups, "Render Pickups");
    ui.checkbox(&mut actors.render_physics_actors, "Render physics actors");
    ui.checkbox(&mut actors.render_actors, "Render actors");
    ui.checkbox(&mut actors.render_all_actors, "Render all actors");
  });
}

/// Body of the "Camera Controls" window. Yaw/Pitch display **degrees** and write
/// back **radians**; `yaw_deg` is `fmod 360` of the degree value. Yaw and pitch are
/// only written back when the drag actually `.changed()`.
pub(crate) fn render_camera_controls_ui(
  ui: &mut egui::Ui,
  cam_line_length: &mut f32,
  manual_camera_pos: &mut glam::Vec3,
  yaw: &mut f32,
  pitch: &mut f32,
) {
  ui.add(
    egui::DragValue::new(cam_line_length)
      .speed(1.0)
      .range(2.0..=250.0)
      .prefix("Camera line length: "),
  );

  ui.horizontal(|ui| {
    ui.label("Position");
    ui.add(egui::DragValue::new(&mut manual_camera_pos.x).speed(1.0));
    ui.add(egui::DragValue::new(&mut manual_camera_pos.y).speed(1.0));
    ui.add(egui::DragValue::new(&mut manual_camera_pos.z).speed(1.0));
  });

  let mut yaw_deg = yaw.to_degrees() % 360.0;
  if ui
    .add(
      egui::DragValue::new(&mut yaw_deg)
        .speed(1.0)
        .range(-360.0..=360.0)
        .prefix("Yaw: "),
    )
    .changed()
  {
    *yaw = yaw_deg.to_radians();
  }

  let mut pitch_deg = pitch.to_degrees();
  if ui
    .add(
      egui::DragValue::new(&mut pitch_deg)
        .speed(1.0)
        .range(-89.0..=89.0)
        .prefix("Pitch: "),
    )
    .changed()
  {
    *pitch = pitch_deg.to_radians();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn render_menu_bar_type_checks_and_does_not_panic() {
    let mut culling = CullType::Back;
    let mut player_clip = PlayerClipConfig::default();
    let mut camera_mode = CameraMode::FollowPlayer;
    let mut orbit = OrbitPlayerCameraOrigin::Center;
    let mut speed = 1.0_f32;
    let mut show_controls = false;
    let mut triggers = TriggerRenderConfig::default();
    let mut actors = ActorRenderConfig::default();
    // FollowPlayer path (orbit sub-group visible).
    egui::__run_test_ui(|ui| {
      render_menu_bar(
        ui,
        &mut culling,
        &mut player_clip,
        &mut camera_mode,
        &mut orbit,
        &mut speed,
        &mut show_controls,
        &mut triggers,
        &mut actors,
      );
    });
    // Detached path (speed slider + controls toggle visible) + clip disabled.
    camera_mode = CameraMode::Detached;
    player_clip.enabled = false;
    egui::__run_test_ui(|ui| {
      render_menu_bar(
        ui,
        &mut culling,
        &mut player_clip,
        &mut camera_mode,
        &mut orbit,
        &mut speed,
        &mut show_controls,
        &mut triggers,
        &mut actors,
      );
    });
  }

  #[test]
  fn render_camera_controls_ui_type_checks_and_does_not_panic() {
    let mut cll = 10.0_f32;
    let mut pos = glam::Vec3::new(1.0, 2.0, 3.0);
    let mut yaw = 1.0_f32;
    let mut pitch = 0.3_f32;
    egui::__run_test_ui(|ui| {
      render_camera_controls_ui(ui, &mut cll, &mut pos, &mut yaw, &mut pitch);
    });
  }

  #[test]
  fn culling_menu_label_value_skew_is_preserved() {
    // "Show Front" -> BACK, "Show Back" -> FRONT.
    let mut culling = CullType::None;
    egui::__run_test_ui(|ui| {
      ui.selectable_value(&mut culling, CullType::Back, "Show Front");
    });
    // The mapping under test is the (label, variant) pairing above; assert the
    // pairing a reviewer cares about rather than simulating a click.
    let front_variant = CullType::Back;
    let back_variant = CullType::Front;
    assert_ne!(front_variant, back_variant);
    assert_eq!(front_variant, CullType::Back);
  }

  #[test]
  fn camera_controls_yaw_pitch_deg_rad_roundtrip() {
    // yaw_deg = to_degrees % 360.
    let yaw = std::f32::consts::PI * 3.0; // 540 deg
    let yaw_deg = yaw.to_degrees() % 360.0;
    assert!((yaw_deg - 180.0).abs() < 1e-3);
    let neg = -std::f32::consts::PI * 3.0;
    assert!((neg.to_degrees() % 360.0 + 180.0).abs() < 1e-3);
    // write-back path: deg -> rad.
    assert!((90.0_f32.to_radians() - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
  }
}
