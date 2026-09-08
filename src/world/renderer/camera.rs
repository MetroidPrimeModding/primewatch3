//! Pure camera math: the `glm::quat(euler)` / `glm::perspective` /
//! `glm::project` ports, and [`compute_camera`] — the camera-setup block of
//! `WorldRenderer::render`, factored out so it's unit-testable without a GPU
//! device.

use glam::{Mat4, Quat, Vec2, Vec3, Vec4};

use super::types::{CameraMode, GameCamera, OrbitPlayerCameraOrigin};

/// The `lookPos.z += …` nudge from the C++ camera setup.
pub(crate) fn orbit_z_nudge(origin: OrbitPlayerCameraOrigin, morphed: bool) -> f32 {
  match origin {
    OrbitPlayerCameraOrigin::Top => {
      if morphed {
        1.4
      } else {
        2.7
      }
    }
    OrbitPlayerCameraOrigin::Center => {
      if morphed {
        0.7
      } else {
        1.35
      }
    }
    OrbitPlayerCameraOrigin::Bottom => 0.0,
  }
}

/// glm's half-angle quaternion constructor `glm::quat(glm::vec3 eulerAngle)`:
/// ```text
/// c = cos(euler * 0.5); s = sin(euler * 0.5);
/// w = c.x*c.y*c.z + s.x*s.y*s.z
/// x = s.x*c.y*c.z - c.x*s.y*s.z
/// y = c.x*s.y*c.z + s.x*c.y*s.z
/// z = c.x*c.y*s.z - s.x*s.y*c.z
/// ```
/// The C++ calls it as `glm::quat(glm::vec3(0, pitch, yaw))`, i.e.
/// `euler.x = 0`, `euler.y = pitch`, `euler.z = yaw`.
pub fn quat_from_euler(euler: Vec3) -> Quat {
  let h = euler * 0.5;
  let (sx, cx) = h.x.sin_cos();
  let (sy, cy) = h.y.sin_cos();
  let (sz, cz) = h.z.sin_cos();
  Quat::from_xyzw(
    sx * cy * cz - cx * sy * sz,
    cx * sy * cz + sx * cy * sz,
    cx * cy * sz - sx * sy * cz,
    cx * cy * cz + sx * sy * sz,
  )
}

/// Wraps `glm::perspective(fov, aspect, zNear, zFar)`.
///
/// TODO: we need to find out where we are using degrees vs radians and fix this
/// NOTE: the C++ passes `fov` (default `45`) straight into `glm::perspective`,
/// whose first parameter is the vertical FOV in **radians** — `45` rad is
/// almost certainly a latent bug in the original. This is ported verbatim: no
/// degrees→radians conversion here.
///
/// Uses glam's DirectX-convention RH projection ([0, 1] clip depth) — the wgpu
/// convention.
fn perspective(fov: f32, aspect: f32, z_near: f32, z_far: f32) -> Mat4 {
  glam::camera::rh::proj::directx::perspective(fov, aspect.max(1e-3), z_near, z_far)
}

/// Pure inputs for [`compute_camera`] — the camera-relevant `WorldRenderer`
/// fields, copied so the math is unit-testable without a GPU device.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CameraParams {
  pub camera_mode: CameraMode,
  pub orbit: OrbitPlayerCameraOrigin,
  pub fov: f32,
  pub aspect: f32,
  pub z_near: f32,
  pub z_far: f32,
  pub pitch: f32,
  pub yaw: f32,
  pub distance: f32,
  pub up: Vec3,
  pub player_is_morphed: bool,
  pub last_known_non_colliding_pos: Vec3,
  pub manual_camera_pos: Vec3,
  pub game_cam: GameCamera,
}

pub(crate) struct CameraResult {
  pub projection: Mat4,
  pub view: Mat4,
  pub eye: Vec3,
  pub manual_camera_pos: Vec3,
}

/// The camera-setup block of `WorldRenderer::render`.
pub(crate) fn compute_camera(p: &CameraParams) -> CameraResult {
  let mut manual_camera_pos = p.manual_camera_pos;
  let (projection, view) = match p.camera_mode {
    CameraMode::FollowPlayer => {
      let proj = perspective(p.fov, p.aspect, p.z_near, p.z_far);
      let angle = quat_from_euler(Vec3::new(0.0, p.pitch, p.yaw));
      let mut look_pos = p.last_known_non_colliding_pos;
      look_pos.z += orbit_z_nudge(p.orbit, p.player_is_morphed);
      // The quat rotates the xyz, the vec4 subtraction truncates to vec3.
      let eye = look_pos - (angle * Vec3::new(p.distance, 0.0, 0.0));
      manual_camera_pos = look_pos;
      (
        proj,
        glam::camera::rh::view::look_at_mat4(eye, look_pos, p.up),
      )
    }
    CameraMode::GameCam => (p.game_cam.perspective, p.game_cam.transform.inverse()),
    CameraMode::Detached => {
      let proj = perspective(p.fov, p.aspect, p.z_near, p.z_far);
      let angle = quat_from_euler(Vec3::new(0.0, p.pitch, p.yaw));
      let eye = p.manual_camera_pos - (angle * Vec3::new(p.distance, 0.0, 0.0));
      (
        proj,
        glam::camera::rh::view::look_at_mat4(eye, p.manual_camera_pos, p.up),
      )
    }
  };
  // Replaces the `glm::decompose(camView, …)` block — only `camEye` is consumed
  // downstream (the shader `viewPos`). The camera-to-world translation is the
  // true eye position.
  let eye = view.inverse().w_axis.truncate();
  CameraResult {
    projection,
    view,
    eye,
    manual_camera_pos,
  }
}

/// `glm::project(obj, view, projection, viewport)` as used by
/// `getScreenspacePosFor*`.
///
/// `clip = projection * view * vec4(pos, 1)`, perspective-divide to NDC, then map
/// to the pixel viewport: `screen.xy = viewport.xy + (ndc.xy + 1) * 0.5 *
/// viewport.zw`. `viewport` is `[x, y, width, height]` in pixels.
///
/// The renderer's projection matrix is glam's DirectX-convention RH perspective
/// ([0, 1] clip depth) rather than GL's [-1, 1] — the x/y screen mapping is
/// identical either way, and callers only consume `.x` / `.y` (the returned `.z`
/// is the raw NDC depth and is unused).
///
/// Returns `None` when the point is on or behind the camera plane (`clip.w <= 0`).
pub(crate) fn project(pos: Vec3, view: Mat4, projection: Mat4, viewport: [f32; 4]) -> Option<Vec3> {
  let clip = projection * view * pos.extend(1.0);
  if clip.w <= 0.0 {
    return None;
  }
  let ndc = clip.truncate() / clip.w;
  Some(Vec3::new(
    viewport[0] + (ndc.x + 1.0) * 0.5 * viewport[2],
    viewport[1] + (ndc.y + 1.0) * 0.5 * viewport[3],
    ndc.z,
  ))
}

/// Inverse of [`project`]: turn a pixel on the world view into a world-space
/// ray `(origin, dir)` (`dir` unit length).
///
/// `screen_px` is **top-left origin, Y-down** (egui image-local pixels) —
/// `project` returns bottom-left-origin Y-up, so this flips Y internally.
/// `viewport` is `[x, y, width, height]` in the same pixels as `screen_px`.
///
/// Unprojects the near (NDC z = 0) and far (NDC z = 1) points through
/// `(projection * view)⁻¹` — the [0, 1] clip-depth convention of the renderer's
/// DirectX-style RH projection. Returns `None` on a degenerate viewport / matrix.
pub(crate) fn unproject_ray(
  screen_px: Vec2,
  view: Mat4,
  projection: Mat4,
  viewport: [f32; 4],
) -> Option<(Vec3, Vec3)> {
  let (w, h) = (viewport[2], viewport[3]);
  if w <= 0.0 || h <= 0.0 {
    return None;
  }
  let ndc_x = (screen_px.x - viewport[0]) / w * 2.0 - 1.0;
  // egui pixels are Y-down from the top; NDC Y is up from the centre.
  let ndc_y = 1.0 - (screen_px.y - viewport[1]) / h * 2.0;

  let inv = (projection * view).inverse();
  let near = inv * Vec4::new(ndc_x, ndc_y, 0.0, 1.0);
  let far = inv * Vec4::new(ndc_x, ndc_y, 1.0, 1.0);
  if near.w.abs() < 1e-9 || far.w.abs() < 1e-9 {
    return None;
  }
  let near = near.truncate() / near.w;
  let far = far.truncate() / far.w;

  let dir = (far - near).normalize_or_zero();
  if dir == Vec3::ZERO {
    return None;
  }
  Some((near, dir))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn base_params() -> CameraParams {
    CameraParams {
      camera_mode: CameraMode::FollowPlayer,
      orbit: OrbitPlayerCameraOrigin::Bottom,
      fov: 45.0,
      aspect: 1.5,
      z_near: 0.1,
      z_far: 10000.0,
      pitch: 0.0,
      yaw: 0.0,
      distance: 10.0,
      up: Vec3::new(0.0, 0.0, 1.0),
      player_is_morphed: false,
      last_known_non_colliding_pos: Vec3::ZERO,
      manual_camera_pos: Vec3::ZERO,
      game_cam: GameCamera::default(),
    }
  }

  fn approx(a: Vec3, b: Vec3) {
    assert!((a - b).length() < 1e-3, "{a:?} != {b:?}");
  }

  #[test]
  fn quat_from_euler_matches_glm_angle_formula() {
    for euler in [
      Vec3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0),
      Vec3::new(0.3, 0.5, 0.7),
      Vec3::new(0.0, -1.2, 2.1),
    ] {
      let h = euler * 0.5;
      let (sx, cx) = h.x.sin_cos();
      let (sy, cy) = h.y.sin_cos();
      let (sz, cz) = h.z.sin_cos();
      let want = Quat::from_xyzw(
        sx * cy * cz - cx * sy * sz,
        cx * sy * cz + sx * cy * sz,
        cx * cy * sz - sx * sy * cz,
        cx * cy * cz + sx * sy * sz,
      );
      let got = quat_from_euler(euler);
      assert!((got.x - want.x).abs() < 1e-6);
      assert!((got.y - want.y).abs() < 1e-6);
      assert!((got.z - want.z).abs() < 1e-6);
      assert!((got.w - want.w).abs() < 1e-6);
    }
  }

  #[test]
  fn quat_from_euler_yaw_only_rotates_about_z() {
    // euler = (0, 0, yaw) -> pure Z rotation.
    let q = quat_from_euler(Vec3::new(0.0, 0.0, std::f32::consts::FRAC_PI_2));
    approx(q * Vec3::X, Vec3::Y);
  }

  #[test]
  fn follow_player_places_eye_behind_look_pos() {
    let mut p = base_params();
    p.last_known_non_colliding_pos = Vec3::new(1.0, 2.0, 3.0);
    let r = compute_camera(&p);
    // yaw=pitch=0 -> angle = identity -> eye = lookPos - (distance,0,0).
    approx(r.eye, Vec3::new(1.0 - 10.0, 2.0, 3.0));
    approx(r.manual_camera_pos, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(
      r.projection,
      perspective(p.fov, p.aspect, p.z_near, p.z_far)
    );
  }

  #[test]
  fn follow_player_center_origin_nudges_look_pos_z() {
    let mut p = base_params();
    p.orbit = OrbitPlayerCameraOrigin::Center;
    p.player_is_morphed = false;
    let r = compute_camera(&p);
    // lookPos.z += 1.35; eye = lookPos - (10,0,0).
    approx(r.eye, Vec3::new(-10.0, 0.0, 1.35));
  }

  #[test]
  fn game_cam_uses_the_read_matrices() {
    let mut p = base_params();
    p.camera_mode = CameraMode::GameCam;
    p.game_cam.perspective = Mat4::from_scale(Vec3::new(2.0, 3.0, 4.0));
    p.game_cam.transform = Mat4::from_translation(Vec3::new(5.0, 6.0, 7.0));
    let r = compute_camera(&p);
    assert_eq!(r.projection, p.game_cam.perspective);
    assert_eq!(r.view, p.game_cam.transform.inverse());
    // eye = view.inverse().w_axis = transform translation.
    approx(r.eye, Vec3::new(5.0, 6.0, 7.0));
  }

  #[test]
  fn detached_orbits_manual_camera_pos_without_z_nudge() {
    let mut p = base_params();
    p.camera_mode = CameraMode::Detached;
    p.orbit = OrbitPlayerCameraOrigin::Center; // ignored in Detached
    p.manual_camera_pos = Vec3::new(4.0, 0.0, 0.0);
    p.distance = 3.0;
    let r = compute_camera(&p);
    approx(r.eye, Vec3::new(1.0, 0.0, 0.0));
  }

  #[test]
  fn project_maps_center_and_corners_to_pixel_viewport() {
    let vp = [0.0, 0.0, 800.0, 600.0];
    // With identity view+proj, the origin sits at NDC (0,0) -> viewport centre.
    let c = project(Vec3::ZERO, Mat4::IDENTITY, Mat4::IDENTITY, vp).unwrap();
    approx(c, Vec3::new(400.0, 300.0, 0.0));
    // NDC (1,1) -> far corner (before the caller's Y flip).
    let corner = project(Vec3::new(1.0, 1.0, 0.0), Mat4::IDENTITY, Mat4::IDENTITY, vp).unwrap();
    approx(corner, Vec3::new(800.0, 600.0, 0.0));
    // NDC (-1,-1) -> origin corner.
    let origin = project(
      Vec3::new(-1.0, -1.0, 0.0),
      Mat4::IDENTITY,
      Mat4::IDENTITY,
      vp,
    )
    .unwrap();
    approx(origin, Vec3::new(0.0, 0.0, 0.0));
  }

  #[test]
  fn project_rejects_points_behind_the_camera() {
    // RH perspective looking down -Z: a point at +Z is behind the camera and
    // projects with clip.w <= 0. Previously this mirrored onto the screen.
    let proj = perspective(45.0, 4.0 / 3.0, 0.1, 1000.0);
    let vp = [0.0, 0.0, 800.0, 600.0];
    assert!(project(Vec3::new(0.0, 0.0, 5.0), Mat4::IDENTITY, proj, vp).is_none());
    assert!(project(Vec3::new(0.0, 0.0, -5.0), Mat4::IDENTITY, proj, vp).is_some());
  }

  #[test]
  fn unproject_ray_inverts_project() {
    let proj = perspective(45.0_f32.to_radians(), 4.0 / 3.0, 0.1, 1000.0);
    let view = glam::camera::rh::view::look_at_mat4(
      Vec3::new(0.0, -10.0, 3.0),
      Vec3::new(1.0, 2.0, 0.5),
      Vec3::Z,
    );
    let vp = [0.0, 0.0, 800.0, 600.0];

    for world in [
      Vec3::new(1.0, 2.0, 0.5),
      Vec3::new(-3.0, 0.0, 2.0),
      Vec3::new(4.0, -1.0, -1.0),
    ] {
      // `project` gives bottom-left-origin Y-up pixels; flip to egui's top-down.
      let s = project(world, view, proj, vp).unwrap();
      let egui_px = Vec2::new(s.x, vp[3] - s.y);

      let (origin, dir) = unproject_ray(egui_px, view, proj, vp).unwrap();
      assert!((dir.length() - 1.0).abs() < 1e-4);
      // `world` must lie on the ray: its rejection from `dir` is ~0.
      let to_world = world - origin;
      let perp = to_world - dir * to_world.dot(dir);
      assert!(perp.length() < 1e-2, "point off ray by {}", perp.length());
    }
  }

  #[test]
  fn unproject_ray_rejects_degenerate_viewport() {
    assert!(
      unproject_ray(
        Vec2::ZERO,
        Mat4::IDENTITY,
        Mat4::IDENTITY,
        [0.0, 0.0, 0.0, 600.0]
      )
      .is_none()
    );
  }

  #[test]
  fn project_perspective_divides_by_w() {
    // A projection that scales w by the point's z; a point at z=2 halves x/y.
    let proj = Mat4::from_cols(
      glam::Vec4::new(1.0, 0.0, 0.0, 0.0),
      glam::Vec4::new(0.0, 1.0, 0.0, 0.0),
      glam::Vec4::new(0.0, 0.0, 1.0, 1.0),
      glam::Vec4::new(0.0, 0.0, 0.0, 0.0),
    );
    let vp = [0.0, 0.0, 200.0, 200.0];
    let s = project(Vec3::new(1.0, 0.0, 2.0), Mat4::IDENTITY, proj, vp).unwrap();
    // clip = (1, 0, 2, 2) -> ndc.x = 0.5 -> screen.x = (0.5+1)*0.5*200 = 150.
    approx(s, Vec3::new(150.0, 100.0, 1.0));
  }
}
