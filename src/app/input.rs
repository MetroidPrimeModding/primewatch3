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
  /// Pointer drag delta over the image since the last frame, in egui points.
  pub(super) drag: (f32, f32),
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
  /// Net WASD/QE contribution for `CameraMode::Detached` (`forward = W - S`,
  /// `right = A - D`, `up = E - Q`).
  pub(super) detached_move: (f32, f32, f32),
}

impl InputState {
  /// Folds accumulated input into an [`InputPlan`].
  /// Pure: the caller applies the plan.
  ///
  /// `world_view` carries last frame's drag/scroll over the "World" image
  /// (see [`WorldViewInput`]).
  pub(super) fn plan(
    &self,
    wants_keyboard: bool,
    camera_mode: CameraMode,
    world_view: WorldViewInput,
  ) -> InputPlan {
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

    // Mouse look + wheel zoom, driven by the "World" image's own drag/scroll
    // response (`world_view`). Yaw uses `yawSpeed = -0.005` (FPS-style:
    // drag right → look right). `scroll` is in egui points (~50/notch)
    wi.cam_pitch = world_view.drag.1 * 0.005;
    wi.cam_yaw = world_view.drag.0 * -0.005;
    wi.cam_zoom = world_view.scroll / 50.0 * -2.0;

    // Keyboard camera control.
    let mut detached_move = (0.0_f32, 0.0_f32, 0.0_f32);
    if !wants_keyboard {
      let down = |k| self.keys_down.contains(&k);
      if down(KeyCode::ArrowUp) {
        wi.cam_pitch += 0.03;
      }
      if down(KeyCode::ArrowDown) {
        wi.cam_pitch -= 0.03;
      }
      if down(KeyCode::ArrowLeft) {
        wi.cam_yaw -= 0.03;
      }
      if down(KeyCode::ArrowRight) {
        wi.cam_yaw += 0.03;
      }
      if down(KeyCode::PageUp) {
        wi.cam_zoom -= 0.5;
      }
      if down(KeyCode::PageDown) {
        wi.cam_zoom += 0.5;
      }
      if camera_mode == CameraMode::Detached {
        let axis = |a, b| i32::from(down(a)) as f32 - i32::from(down(b)) as f32;
        detached_move = (
          axis(KeyCode::KeyW, KeyCode::KeyS),
          axis(KeyCode::KeyA, KeyCode::KeyD),
          axis(KeyCode::KeyE, KeyCode::KeyQ),
        );
      }
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

  fn state() -> InputState {
    InputState::default()
  }

  #[test]
  fn world_drag_drives_yaw_pitch() {
    let s = state();
    let wv = WorldViewInput {
      drag: (10.0, 4.0),
      scroll: 0.0,
    };
    let plan = s.plan(false, CameraMode::FollowPlayer, wv);
    // Rightward drag → negative yaw (FPS-style: look right).
    assert!((plan.world_input.cam_yaw - (10.0 * -0.005)).abs() < 1e-6);
    assert!((plan.world_input.cam_pitch - (4.0 * 0.005)).abs() < 1e-6);
  }

  #[test]
  fn world_scroll_drives_zoom() {
    let s = state();
    let wv = WorldViewInput {
      drag: (0.0, 0.0),
      scroll: 50.0,
    };
    let plan = s.plan(false, CameraMode::FollowPlayer, wv);
    assert!((plan.world_input.cam_zoom - (50.0 / 50.0 * -2.0)).abs() < 1e-6);
  }

  #[test]
  fn no_world_view_input_means_no_camera_motion() {
    let s = state();
    let plan = s.plan(false, CameraMode::FollowPlayer, WorldViewInput::default());
    assert_eq!(plan.world_input.cam_yaw, 0.0);
    assert_eq!(plan.world_input.cam_pitch, 0.0);
    assert_eq!(plan.world_input.cam_zoom, 0.0);
  }

  #[test]
  fn shift_and_ctrl_digits_record_and_clear_ghosts() {
    let mut s = state();
    s.keys_down.insert(KeyCode::Digit3);
    s.modifiers = ModifiersState::SHIFT;
    let plan = s.plan(false, CameraMode::FollowPlayer, WorldViewInput::default());
    assert_eq!(plan.ghost_record, [false, false, true, false, false]);
    assert_eq!(plan.ghost_clear, [false; 5]);

    s.modifiers = ModifiersState::CONTROL;
    let plan = s.plan(false, CameraMode::FollowPlayer, WorldViewInput::default());
    assert_eq!(plan.ghost_clear, [false, false, true, false, false]);
    assert_eq!(plan.ghost_record, [false; 5]);
  }

  #[test]
  fn wasd_only_moves_in_detached_mode_and_arrows_always_work() {
    let mut s = state();
    s.keys_down.insert(KeyCode::KeyW);
    s.keys_down.insert(KeyCode::ArrowLeft);

    let plan = s.plan(false, CameraMode::FollowPlayer, WorldViewInput::default());
    assert_eq!(plan.detached_move, (0.0, 0.0, 0.0));
    assert!((plan.world_input.cam_yaw - -0.03).abs() < 1e-6);

    let plan = s.plan(false, CameraMode::Detached, WorldViewInput::default());
    assert_eq!(plan.detached_move, (1.0, 0.0, 0.0));
  }

  #[test]
  fn keyboard_capture_blocks_movement_keys() {
    let mut s = state();
    s.keys_down.insert(KeyCode::KeyW);
    s.keys_down.insert(KeyCode::ArrowUp);
    let plan = s.plan(true, CameraMode::Detached, WorldViewInput::default());
    assert_eq!(plan.detached_move, (0.0, 0.0, 0.0));
    assert_eq!(plan.world_input.cam_pitch, 0.0);
  }
}
