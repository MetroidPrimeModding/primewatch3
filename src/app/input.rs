//! Raw winit input accumulation and its per-frame reduction into a
//! [`crate::world::renderer::WorldInput`] plus camera/ghost side effects.

use std::collections::HashSet;

use winit::keyboard::{KeyCode, ModifiersState};

use crate::world::renderer::{CameraMode, WorldInput};

/// The five ghost-record hotkeys
pub(super) const GHOST_KEYS: [KeyCode; 5] = [
  KeyCode::Digit1,
  KeyCode::Digit2,
  KeyCode::Digit3,
  KeyCode::Digit4,
  KeyCode::Digit5,
];

/// Raw winit input accumulated between frames, then folded into a [`WorldInput`]
/// (plus camera / ghost side effects) at the top of each frame.
#[derive(Default)]
pub(super) struct InputState {
  pub(super) keys_down: HashSet<KeyCode>,
  pub(super) modifiers: ModifiersState,
}

/// One-frame-lagged interaction state of the "World" image widget, produced by
/// the egui pass and consumed by [`InputState::plan`] on the next frame (same
/// lag pattern as `world_view_px`).
#[derive(Default, Clone, Copy)]
pub(super) struct WorldViewInput {
  /// Primary-button pointer drag delta over the image since the last frame,
  /// in egui points. Drives camera look (yaw/pitch).
  pub(super) drag: (f32, f32),
  /// Middle-button pointer drag delta over the image since the last frame, in
  /// egui points. Pans the detached camera (see [`InputState::plan`]).
  pub(super) middle_drag: (f32, f32),
  /// Scroll delta while hovering the image, in egui points.
  pub(super) scroll: f32,
}

/// Result of [`InputState::plan`] — a [`WorldInput`] plus the direct
/// `worldRenderer` mutations `processInput` performs (ghost record/clear,
/// detached-camera movement) and the resolved mouse-capture state.
pub(super) struct InputPlan {
  pub(super) world_input: WorldInput,
  pub(super) ghost_record: [bool; 5],
  pub(super) ghost_clear: [bool; 5],
  /// Net WASD/QE (tripled while Shift is held) plus wheel-dolly plus
  /// middle-drag-pan contribution for `CameraMode::Detached` (`forward = W -
  /// S` plus wheel, `right = A - D` plus pan, `up = E - Q` plus pan).
  pub(super) detached_move: (f32, f32, f32),
}

impl InputState {
  /// Folds accumulated input into an [`InputPlan`].
  /// Pure: the caller applies the plan.
  ///
  /// `world_view` carries last frame's drag/scroll over the "World" image
  /// (see [`WorldViewInput`]).
  /// `dt` is the real time (seconds) since the last frame — every held-key
  /// increment below is a "units per frame" constant tuned by feel at
  /// ~60 FPS, so it's multiplied by `dt * 60` to become "units per second"
  /// instead: uncapped/high-refresh frame rates no longer fly the camera
  /// faster than someone at 60 FPS. Mouse drag/scroll deltas are already a
  /// measured amount of physical input since the last frame, not a per-frame
  /// constant, so they're left alone.
  pub(super) fn plan(
    &self,
    wants_keyboard: bool,
    camera_mode: CameraMode,
    world_view: WorldViewInput,
    dt: f32,
  ) -> InputPlan {
    let dt_scale = dt * 60.0;
    // Shift sprints the arrow-key look and WASDQE fly-cam move (not the
    // ghost-record Shift+digit chord below, which is a different keyset).
    const SPRINT_MULTIPLIER: f32 = 3.0;
    let held_scale = if self.modifiers.shift_key() {
      dt_scale * SPRINT_MULTIPLIER
    } else {
      dt_scale
    };
    let mut wi = WorldInput::default();

    // Shift+N records ghost N, Ctrl+N clears it.
    let mut ghost_record = [false; 5];
    let mut ghost_clear = [false; 5];
    for (i, key) in GHOST_KEYS.iter().enumerate() {
      if self.keys_down.contains(key) {
        if self.modifiers.shift_key() {
          ghost_record[i] = true;
        } else if self.modifiers.control_key() {
          ghost_clear[i] = true;
        }
      }
    }

    // Mouse look, driven by the "World" image's own drag response
    // (`world_view`). Yaw uses `yawSpeed = -0.005` (FPS-style: drag right →
    // look right).
    wi.cam_pitch = world_view.drag.1 * 0.005;
    wi.cam_yaw = world_view.drag.0 * -0.005;

    // The wheel/PageUp/PageDown "zoom" axis: for the orbit cameras this is
    // FOV/distance zoom (`wi.cam_zoom`, negative = zoom in); in `Detached` it
    // instead dollies the first-person camera forward/back, so it's folded
    // into `detached_move` below rather than `wi.cam_zoom`. `scroll` is in
    // egui points (~50/notch).
    let mut zoom = world_view.scroll / 50.0 * -2.0;

    // Keyboard camera control.
    let mut detached_move = (0.0_f32, 0.0_f32, 0.0_f32);
    if !wants_keyboard {
      let down = |k| self.keys_down.contains(&k);
      if down(KeyCode::ArrowUp) {
        wi.cam_pitch += 0.03 * held_scale;
      }
      if down(KeyCode::ArrowDown) {
        wi.cam_pitch -= 0.03 * held_scale;
      }
      if down(KeyCode::ArrowLeft) {
        wi.cam_yaw -= 0.03 * held_scale;
      }
      if down(KeyCode::ArrowRight) {
        wi.cam_yaw += 0.03 * held_scale;
      }
      if down(KeyCode::PageUp) {
        zoom -= 0.5 * dt_scale;
      }
      if down(KeyCode::PageDown) {
        zoom += 0.5 * dt_scale;
      }
      if camera_mode == CameraMode::Detached {
        let axis = |a, b| i32::from(down(a)) as f32 - i32::from(down(b)) as f32;
        detached_move = (
          axis(KeyCode::KeyW, KeyCode::KeyS) * held_scale,
          axis(KeyCode::KeyA, KeyCode::KeyD) * held_scale,
          axis(KeyCode::KeyE, KeyCode::KeyQ) * held_scale,
        );
      }
    }

    if camera_mode == CameraMode::Detached {
      // `zoom < 0` means "zoom in" on the orbit cameras, which maps to "move
      // forward" here.
      const ZOOM_SPEED: f32 = 2.0;
      detached_move.0 -= zoom * ZOOM_SPEED;

      // Middle-drag pans the first-person camera (screen-relative
      // strafe/rise); mouse-driven so it isn't gated by `wants_keyboard`.
      const PAN_SPEED: f32 = 0.1;
      detached_move.1 += world_view.middle_drag.0 * PAN_SPEED;
      detached_move.2 += world_view.middle_drag.1 * PAN_SPEED;
    } else {
      wi.cam_zoom = zoom;
    }

    InputPlan {
      world_input: wi,
      ghost_record,
      ghost_clear,
      detached_move,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// A 60 FPS frame, i.e. `dt_scale == 1.0` — keeps the held-key assertions
  /// below numerically identical to the pre-dt-scaling constants.
  const DT_60FPS: f32 = 1.0 / 60.0;

  fn state() -> InputState {
    InputState::default()
  }

  #[test]
  fn world_drag_drives_yaw_pitch() {
    let s = state();
    let wv = WorldViewInput {
      drag: (10.0, 4.0),
      ..Default::default()
    };
    let plan = s.plan(false, CameraMode::FollowPlayer, wv, DT_60FPS);
    // Rightward drag → negative yaw (FPS-style: look right).
    assert!((plan.world_input.cam_yaw - (10.0 * -0.005)).abs() < 1e-6);
    assert!((plan.world_input.cam_pitch - (4.0 * 0.005)).abs() < 1e-6);
  }

  #[test]
  fn world_scroll_drives_zoom() {
    let s = state();
    let wv = WorldViewInput {
      scroll: 50.0,
      ..Default::default()
    };
    let plan = s.plan(false, CameraMode::FollowPlayer, wv, DT_60FPS);
    assert!((plan.world_input.cam_zoom - (50.0 / 50.0 * -2.0)).abs() < 1e-6);
  }

  #[test]
  fn no_world_view_input_means_no_camera_motion() {
    let s = state();
    let plan = s.plan(
      false,
      CameraMode::FollowPlayer,
      WorldViewInput::default(),
      DT_60FPS,
    );
    assert_eq!(plan.world_input.cam_yaw, 0.0);
    assert_eq!(plan.world_input.cam_pitch, 0.0);
    assert_eq!(plan.world_input.cam_zoom, 0.0);
  }

  #[test]
  fn shift_and_ctrl_digits_record_and_clear_ghosts() {
    let mut s = state();
    s.keys_down.insert(KeyCode::Digit3);
    s.modifiers = ModifiersState::SHIFT;
    let plan = s.plan(
      false,
      CameraMode::FollowPlayer,
      WorldViewInput::default(),
      DT_60FPS,
    );
    assert_eq!(plan.ghost_record, [false, false, true, false, false]);
    assert_eq!(plan.ghost_clear, [false; 5]);

    s.modifiers = ModifiersState::CONTROL;
    let plan = s.plan(
      false,
      CameraMode::FollowPlayer,
      WorldViewInput::default(),
      DT_60FPS,
    );
    assert_eq!(plan.ghost_clear, [false, false, true, false, false]);
    assert_eq!(plan.ghost_record, [false; 5]);
  }

  #[test]
  fn wasd_only_moves_in_detached_mode_and_arrows_always_work() {
    let mut s = state();
    s.keys_down.insert(KeyCode::KeyW);
    s.keys_down.insert(KeyCode::ArrowLeft);

    let plan = s.plan(
      false,
      CameraMode::FollowPlayer,
      WorldViewInput::default(),
      DT_60FPS,
    );
    assert_eq!(plan.detached_move, (0.0, 0.0, 0.0));
    assert!((plan.world_input.cam_yaw - -0.03).abs() < 1e-6);

    let plan = s.plan(
      false,
      CameraMode::Detached,
      WorldViewInput::default(),
      DT_60FPS,
    );
    assert_eq!(plan.detached_move, (1.0, 0.0, 0.0));
  }

  #[test]
  fn keyboard_capture_blocks_movement_keys() {
    let mut s = state();
    s.keys_down.insert(KeyCode::KeyW);
    s.keys_down.insert(KeyCode::ArrowUp);
    let plan = s.plan(
      true,
      CameraMode::Detached,
      WorldViewInput::default(),
      DT_60FPS,
    );
    assert_eq!(plan.detached_move, (0.0, 0.0, 0.0));
    assert_eq!(plan.world_input.cam_pitch, 0.0);
  }

  #[test]
  fn middle_drag_pans_detached_camera_but_not_other_modes() {
    let s = state();
    let wv = WorldViewInput {
      middle_drag: (10.0, 20.0),
      ..Default::default()
    };
    let plan = s.plan(false, CameraMode::Detached, wv, DT_60FPS);
    assert_eq!(plan.detached_move, (0.0, 10.0 * 0.1, 20.0 * 0.1));

    // Not gated by `wants_keyboard` — mouse-driven.
    let plan = s.plan(true, CameraMode::Detached, wv, DT_60FPS);
    assert_eq!(plan.detached_move, (0.0, 10.0 * 0.1, 20.0 * 0.1));

    // No effect outside Detached mode.
    let plan = s.plan(false, CameraMode::FollowPlayer, wv, DT_60FPS);
    assert_eq!(plan.detached_move, (0.0, 0.0, 0.0));
  }

  #[test]
  fn wheel_dollies_forward_in_detached_but_zooms_elsewhere() {
    let s = state();
    let wv = WorldViewInput {
      scroll: 50.0, // one notch -> zoom = 50.0/50.0*-2.0 = -2.0 ("zoom in")
      ..Default::default()
    };

    // Detached: "zoom in" (-2.0) becomes "move forward" (+2.0 * ZOOM_SPEED),
    // and cam_zoom is left untouched (there's no FOV/distance to zoom here).
    let plan = s.plan(false, CameraMode::Detached, wv, DT_60FPS);
    assert_eq!(plan.detached_move, (4.0, 0.0, 0.0));
    assert_eq!(plan.world_input.cam_zoom, 0.0);

    // Everywhere else the wheel still drives cam_zoom, not movement.
    let plan = s.plan(false, CameraMode::FollowPlayer, wv, DT_60FPS);
    assert_eq!(plan.detached_move, (0.0, 0.0, 0.0));
    assert!((plan.world_input.cam_zoom - -2.0).abs() < 1e-6);
  }

  #[test]
  fn held_key_motion_scales_with_frame_time() {
    // The bug this fixes: at a fixed per-frame constant, a higher frame rate
    // (smaller `dt`) meant more `plan()` calls per second and thus a faster
    // camera. Scaled by `dt`, a quarter-duration frame should produce a
    // quarter of the 60fps-reference motion, so the *rate* (units/sec) is
    // frame-rate independent instead of the *per-frame step*.
    let mut s = state();
    s.keys_down.insert(KeyCode::ArrowRight);
    s.keys_down.insert(KeyCode::KeyW);

    let plan_60fps = s.plan(
      false,
      CameraMode::Detached,
      WorldViewInput::default(),
      DT_60FPS,
    );
    let plan_240fps = s.plan(
      false,
      CameraMode::Detached,
      WorldViewInput::default(),
      DT_60FPS / 4.0,
    );

    assert!((plan_240fps.world_input.cam_yaw - plan_60fps.world_input.cam_yaw / 4.0).abs() < 1e-6);
    assert!((plan_240fps.detached_move.0 - plan_60fps.detached_move.0 / 4.0).abs() < 1e-6);

    // Mouse-driven deltas (middle-drag/scroll) are a measured amount of
    // physical input, not a per-frame constant — they must NOT be rescaled.
    let wv = WorldViewInput {
      middle_drag: (10.0, 0.0),
      ..Default::default()
    };
    let pan_60fps = s.plan(false, CameraMode::Detached, wv, DT_60FPS);
    let pan_240fps = s.plan(false, CameraMode::Detached, wv, DT_60FPS / 4.0);
    assert_eq!(pan_60fps.detached_move.1, pan_240fps.detached_move.1);
  }

  #[test]
  fn shift_triples_arrow_look_and_wasd_move_speed() {
    let mut s = state();
    s.keys_down.insert(KeyCode::ArrowRight);
    s.keys_down.insert(KeyCode::KeyW);

    let walk = s.plan(
      false,
      CameraMode::Detached,
      WorldViewInput::default(),
      DT_60FPS,
    );
    s.modifiers = ModifiersState::SHIFT;
    let sprint = s.plan(
      false,
      CameraMode::Detached,
      WorldViewInput::default(),
      DT_60FPS,
    );

    assert!((sprint.world_input.cam_yaw - walk.world_input.cam_yaw * 3.0).abs() < 1e-6);
    assert!((sprint.detached_move.0 - walk.detached_move.0 * 3.0).abs() < 1e-6);
  }

  #[test]
  fn shift_does_not_boost_middle_drag_pan() {
    let mut s = state();
    let wv = WorldViewInput {
      middle_drag: (10.0, 20.0),
      ..Default::default()
    };
    let walk_pan = s.plan(false, CameraMode::Detached, wv, DT_60FPS);
    s.modifiers = ModifiersState::SHIFT;
    let sprint_pan = s.plan(false, CameraMode::Detached, wv, DT_60FPS);
    assert_eq!(walk_pan.detached_move, sprint_pan.detached_move);
  }

  #[test]
  fn shift_does_not_boost_page_up_down_zoom_dolly() {
    let mut s = state();
    s.keys_down.insert(KeyCode::PageUp);

    let walk = s.plan(
      false,
      CameraMode::Detached,
      WorldViewInput::default(),
      DT_60FPS,
    );
    s.modifiers = ModifiersState::SHIFT;
    let sprint = s.plan(
      false,
      CameraMode::Detached,
      WorldViewInput::default(),
      DT_60FPS,
    );
    assert_eq!(walk.detached_move, sprint.detached_move);
  }
}
