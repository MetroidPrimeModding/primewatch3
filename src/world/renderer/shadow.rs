//! Pure math for the player shadow map: azimuth/elevation -> direction (shared
//! by the scene light and the, optionally independent, shadow direction) and
//! the light's view-projection matrix. Split out from [`super::gpu`] so it's
//! testable without a GPU device, same rationale as [`super::camera`].

use glam::{Mat4, Vec3};

/// Converts an azimuth/elevation pair (radians) to a unit direction, Z-up.
/// `elevation` is measured from the horizontal plane (`0` = level with the
/// horizon, `FRAC_PI_2` = straight up); `azimuth` rotates around Z and is
/// irrelevant at `elevation == +/-FRAC_PI_2`. Shared by the scene light
/// (`WorldRenderer::light_azimuth` / `light_elevation`) and the shadow's own,
/// optionally independent, direction (`ShadowConfig::azimuth` / `elevation`)
/// so "straight down" is exactly representable (`elevation = FRAC_PI_2` gives
/// exactly `Vec3::Z`, no floating-point drift from a stored vector).
pub(super) fn dir_from_azimuth_elevation(azimuth: f32, elevation: f32) -> Vec3 {
  let (el_sin, el_cos) = elevation.sin_cos();
  let (az_sin, az_cos) = azimuth.sin_cos();
  Vec3::new(el_cos * az_cos, el_cos * az_sin, el_sin)
}

/// Distance from the light-space eye to the player center along `light_dir`.
/// Only needs to clear the player's own bounding radius; the orthographic
/// frustum (not this distance) determines what the shadow map can "see".
const LIGHT_DISTANCE: f32 = 8.0;

/// Extra depth budget past the player center, so ground well below the player
/// (slopes, pits) still falls inside the light's far plane and can receive a
/// shadow test instead of being silently treated as unshadowed.
const FAR_MARGIN: f32 = 30.0;

/// Builds the light's view-projection matrix for the player shadow map: an
/// orthographic frustum of `half_extent` centered on `player_center`, looking
/// along `light_dir` -- either the scene light itself or `ShadowConfig`'s own
/// independent direction; the caller picks which. `up` falls back to world X
/// when `light_dir` is nearly vertical, avoiding a
/// degenerate look-at. A zero `light_dir` (shouldn't happen in practice, since
/// callers normalize a fixed default) returns `Mat4::IDENTITY` rather than
/// dividing by zero.
pub(super) fn player_light_view_proj(
  player_center: Vec3,
  light_dir: Vec3,
  half_extent: f32,
) -> Mat4 {
  let light_dir = light_dir.normalize_or_zero();
  if light_dir == Vec3::ZERO {
    return Mat4::IDENTITY;
  }
  let up = if light_dir.z.abs() > 0.99 {
    Vec3::X
  } else {
    Vec3::Z
  };
  // `light_dir` points from a surface *toward* the light (same convention
  // `fs_mesh` uses for `dot(normal, light_dir)`), so the light itself sits on
  // the `+light_dir` side of the player, looking back down `-light_dir`.
  let eye = player_center + light_dir * LIGHT_DISTANCE;
  let view = glam::camera::rh::view::look_at_mat4(eye, player_center, up);
  let half_extent = half_extent.max(0.1);
  let proj = glam::camera::rh::proj::directx::orthographic(
    -half_extent,
    half_extent,
    -half_extent,
    half_extent,
    0.1,
    LIGHT_DISTANCE + FAR_MARGIN,
  );
  proj * view
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn elevation_90_is_exactly_straight_up_regardless_of_azimuth() {
    for az_deg in [0.0f32, 37.0, 90.0, 200.0, -160.0] {
      let d = dir_from_azimuth_elevation(az_deg.to_radians(), std::f32::consts::FRAC_PI_2);
      assert!((d - Vec3::Z).length() < 1e-5, "azimuth {az_deg} -> {d:?}");
    }
  }

  #[test]
  fn zero_elevation_is_level_with_the_horizon() {
    let d = dir_from_azimuth_elevation(0.3, 0.0);
    assert!(d.z.abs() < 1e-6);
    assert!((d.length() - 1.0).abs() < 1e-6);
  }

  #[test]
  fn dir_from_azimuth_elevation_is_always_unit_length() {
    for az in [-3.0, -1.0, 0.0, 1.5, 4.2] {
      for el in [-1.5, -0.5, 0.0, 0.7, 1.5] {
        let d = dir_from_azimuth_elevation(az, el);
        assert!((d.length() - 1.0).abs() < 1e-5, "az={az} el={el} -> {d:?}");
      }
    }
  }

  #[test]
  fn player_center_projects_to_ndc_origin_and_mid_depth() {
    let center = Vec3::new(3.0, -2.0, 5.0);
    // Not near-vertical, so `up` stays world Z (no axis-swap fallback).
    let m = player_light_view_proj(center, Vec3::new(0.0, 1.0, 0.0), 2.5);
    let clip = m * center.extend(1.0);
    assert!(clip.x.abs() < 1e-4);
    assert!(clip.y.abs() < 1e-4);
    // Player sits exactly `LIGHT_DISTANCE` from the light eye -> NDC z ==
    // (LIGHT_DISTANCE - near) / (far - near) for a `directx`-style [0,1]
    // depth-range orthographic projection (linear in view-space depth).
    let near = 0.1;
    let far = LIGHT_DISTANCE + FAR_MARGIN;
    let expected_z = (LIGHT_DISTANCE - near) / (far - near);
    assert!((clip.z - expected_z).abs() < 1e-4);
    assert!((clip.w - 1.0).abs() < 1e-6);
  }

  #[test]
  fn point_offset_along_light_perpendicular_axis_moves_in_ndc_x() {
    // Light along +Y (not near-vertical, so `up` stays world Z): the
    // perpendicular/"right" axis in light-view space is world X.
    let center = Vec3::ZERO;
    let half_extent = 2.5;
    let m = player_light_view_proj(center, Vec3::new(0.0, 1.0, 0.0), half_extent);
    let side = Vec3::new(half_extent, 0.0, 0.0);
    let clip = m * side.extend(1.0);
    // Sign depends on `look_at_mat4`'s internal handedness convention; what
    // matters here is that a point `half_extent` off-axis lands exactly on
    // the frustum edge, not that it's specifically the `+1` edge.
    assert!(
      (clip.x.abs() - 1.0).abs() < 1e-4,
      "expected +half_extent -> NDC x=+/-1, got {}",
      clip.x
    );
  }

  #[test]
  fn nearly_vertical_light_falls_back_to_x_up_without_panicking() {
    let m = player_light_view_proj(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 2.5);
    assert!(m.is_finite());
  }

  #[test]
  fn zero_light_dir_returns_identity() {
    assert_eq!(
      player_light_view_proj(Vec3::ONE, Vec3::ZERO, 2.5),
      Mat4::IDENTITY
    );
  }
}
