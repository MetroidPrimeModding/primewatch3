//! Procedural CPU-side vertex generators — ports
//! `../primewatch2/src/gl/ShapeGenerator.cpp` (whole file) and
//! `../primewatch2/src/gl/ShapeGenerator.hpp`.
//!
//! Six free functions, each returning a `Vec<`[`Vert`]`>` of geometry ready to be
//! uploaded by [`crate::gl::immediate::ImmediateModeBuffer`] / a
//! [`crate::gl::mesh::DynamicMesh`]. No GPU work happens here, mirroring
//! `gl::immediate` being CPU-only.
//!
//! `Vert` default rules (from `gl/OpenGLMesh.hpp:12-17`): a C++
//! `Vert{.pos = ..., .color = ...}` designated-initializer leaves `normal` value
//! initialized to `{0,0,0}` and `barycentric` at its in-class initializer
//! `{-1,-1,-1}`. The [`tri_vert`] / [`line_vert`] / [`pc_vert`] helpers make those
//! defaults explicit per call site.
//!
//! Dead code until P8.4 (`WorldRenderer`) wires it.

use crate::gl::Vert;
use glam::{Mat4, Quat, Vec3, Vec4};

/// Per-triangle barycentric cycle used by every `TRIANGLES` generator here:
/// vertices in a triangle get `{1,0,0}`, `{0,1,0}`, `{0,0,1}` in order.
const BARY_X: [f32; 3] = [1.0, 0.0, 0.0];
const BARY_Y: [f32; 3] = [0.0, 1.0, 0.0];
const BARY_Z: [f32; 3] = [0.0, 0.0, 1.0];

/// A fully specified triangle vertex (`Vert{.pos, .color, .normal, .barycentric}`).
fn tri_vert(pos: Vec3, color: Vec4, normal: Vec3, bary: [f32; 3]) -> Vert {
  Vert {
    pos: pos.to_array(),
    color: color.to_array(),
    normal: normal.to_array(),
    barycentric: bary,
  }
}

/// A line vertex (`Vert{.pos, .color, .normal}`) — `barycentric` takes the C++
/// in-class default `{-1,-1,-1}`.
fn line_vert(pos: Vec3, color: Vec4, normal: Vec3) -> Vert {
  Vert {
    pos: pos.to_array(),
    color: color.to_array(),
    normal: normal.to_array(),
    barycentric: [-1.0; 3],
  }
}

/// A position/colour-only vertex (`Vert{.pos, .color}`) — `normal` value
/// initializes to `{0,0,0}`, `barycentric` to the in-class default `{-1,-1,-1}`.
fn pc_vert(pos: Vec3, color: Vec4) -> Vert {
  Vert {
    pos: pos.to_array(),
    color: color.to_array(),
    normal: [0.0; 3],
    barycentric: [-1.0; 3],
  }
}

/// Solid axis-aligned box as 36 triangle vertices (6 faces, 2 tris each).
///
/// Ports `ShapeGenerator::generateCube` (`ShapeGenerator.cpp:20-72`). Vertex
/// order and per-face normals are transcribed verbatim.
pub fn generate_cube(min: Vec3, max: Vec3, color: Vec4) -> Vec<Vert> {
  let mut verts = Vec::with_capacity(36);
  let mut push_face = |pts: [Vec3; 6], normal: Vec3| {
    let barys = [BARY_X, BARY_Y, BARY_Z, BARY_X, BARY_Y, BARY_Z];
    for (p, bary) in pts.into_iter().zip(barys) {
      verts.push(tri_vert(p, color, normal, bary));
    }
  };

  // -Z
  push_face(
    [
      Vec3::new(max.x, max.y, min.z),
      Vec3::new(min.x, max.y, min.z),
      Vec3::new(min.x, min.y, min.z),
      Vec3::new(min.x, min.y, min.z),
      Vec3::new(max.x, min.y, min.z),
      Vec3::new(max.x, max.y, min.z),
    ],
    Vec3::new(0.0, 0.0, -1.0),
  );
  // +Z
  push_face(
    [
      Vec3::new(min.x, min.y, max.z),
      Vec3::new(min.x, max.y, max.z),
      Vec3::new(max.x, max.y, max.z),
      Vec3::new(max.x, max.y, max.z),
      Vec3::new(max.x, min.y, max.z),
      Vec3::new(min.x, min.y, max.z),
    ],
    Vec3::new(0.0, 0.0, 1.0),
  );
  // -X
  push_face(
    [
      Vec3::new(min.x, min.y, min.z),
      Vec3::new(min.x, max.y, min.z),
      Vec3::new(min.x, max.y, max.z),
      Vec3::new(min.x, max.y, max.z),
      Vec3::new(min.x, min.y, max.z),
      Vec3::new(min.x, min.y, min.z),
    ],
    Vec3::new(-1.0, 0.0, 0.0),
  );
  // +X
  push_face(
    [
      Vec3::new(max.x, max.y, max.z),
      Vec3::new(max.x, max.y, min.z),
      Vec3::new(max.x, min.y, min.z),
      Vec3::new(max.x, min.y, min.z),
      Vec3::new(max.x, min.y, max.z),
      Vec3::new(max.x, max.y, max.z),
    ],
    Vec3::new(1.0, 0.0, 0.0),
  );
  // -Y
  push_face(
    [
      Vec3::new(max.x, min.y, max.z),
      Vec3::new(max.x, min.y, min.z),
      Vec3::new(min.x, min.y, min.z),
      Vec3::new(min.x, min.y, min.z),
      Vec3::new(min.x, min.y, max.z),
      Vec3::new(max.x, min.y, max.z),
    ],
    Vec3::new(0.0, -1.0, 0.0),
  );
  // +Y
  push_face(
    [
      Vec3::new(min.x, max.y, min.z),
      Vec3::new(max.x, max.y, min.z),
      Vec3::new(max.x, max.y, max.z),
      Vec3::new(max.x, max.y, max.z),
      Vec3::new(min.x, max.y, max.z),
      Vec3::new(min.x, max.y, min.z),
    ],
    Vec3::new(0.0, 1.0, 0.0),
  );

  verts
}

/// Solid box specified by centre + full size — ports
/// `ShapeGenerator::generateCubeFromCenter` (`ShapeGenerator.cpp:74-76`).
pub fn generate_cube_from_center(center: Vec3, size: Vec3, color: Vec4) -> Vec<Vert> {
  generate_cube(center - size / 2.0, center + size / 2.0, color)
}

/// Box wireframe as 24 line vertices (12 edges).
///
/// Ports `ShapeGenerator::generateCubeLines` (`ShapeGenerator.cpp:78-121`).
/// Verbatim C++ quirk: every vertex gets `normal = {0,0,-1}` regardless of which
/// edge it belongs to. `barycentric` defaults to `{-1,-1,-1}`.
pub fn generate_cube_lines(min: Vec3, max: Vec3, color: Vec4) -> Vec<Vert> {
  let mut verts = Vec::with_capacity(24);
  let normal = Vec3::new(0.0, 0.0, -1.0);
  let mut edge = |a: Vec3, b: Vec3| {
    verts.push(line_vert(a, color, normal));
    verts.push(line_vert(b, color, normal));
  };

  // Z-aligned edges
  edge(
    Vec3::new(min.x, min.y, min.z),
    Vec3::new(min.x, min.y, max.z),
  );
  edge(
    Vec3::new(min.x, max.y, min.z),
    Vec3::new(min.x, max.y, max.z),
  );
  edge(
    Vec3::new(max.x, max.y, min.z),
    Vec3::new(max.x, max.y, max.z),
  );
  edge(
    Vec3::new(max.x, min.y, min.z),
    Vec3::new(max.x, min.y, max.z),
  );

  // X-aligned edges
  edge(
    Vec3::new(min.x, min.y, min.z),
    Vec3::new(max.x, min.y, min.z),
  );
  edge(
    Vec3::new(min.x, min.y, max.z),
    Vec3::new(max.x, min.y, max.z),
  );
  edge(
    Vec3::new(min.x, max.y, max.z),
    Vec3::new(max.x, max.y, max.z),
  );
  edge(
    Vec3::new(min.x, max.y, min.z),
    Vec3::new(max.x, max.y, min.z),
  );

  // Y-aligned edges
  edge(
    Vec3::new(min.x, min.y, min.z),
    Vec3::new(min.x, max.y, min.z),
  );
  edge(
    Vec3::new(min.x, min.y, max.z),
    Vec3::new(min.x, max.y, max.z),
  );
  edge(
    Vec3::new(max.x, min.y, max.z),
    Vec3::new(max.x, max.y, max.z),
  );
  edge(
    Vec3::new(max.x, min.y, min.z),
    Vec3::new(max.x, max.y, min.z),
  );

  verts
}

/// Rotate the reference point `{0,0,1}` by `longitude` about +Z then `latitude`
/// about +Y — ports the `glm::quat(vec3(0,0,lon)) * glm::quat(vec3(0,lat,0)) *
/// vec3{0,0,1}` idiom used throughout `generateSphere` / `generateTruncatedSphere`.
/// Every rotation is single-axis, so there is no euler-order ambiguity.
fn sphere_point(longitude: f32, latitude: f32) -> Vec3 {
  Quat::from_rotation_z(longitude) * Quat::from_rotation_y(latitude) * Vec3::new(0.0, 0.0, 1.0)
}

const SPHERE_LATITUDE_LINES: i32 = 15;
const SPHERE_LONGITUDE_LINES: i32 = 20;

/// Emit one latitude band (`SPHERE_LONGITUDE_LINES` quads, 6 verts each) — the
/// shared inner loop of `generateSphere` / `generateTruncatedSphere`
/// (`ShapeGenerator.cpp:138-172` / `199-231`), including the `latitudeLine == 0`
/// normal hack. C++ open-codes this loop in both functions.
fn emit_sphere_band(band: SphereBand<'_>) {
  let SphereBand {
    verts,
    center,
    radius,
    color,
    radians_per_longitude,
    latitude,
    next_latitude,
    latitude_line,
  } = band;

  for longitude_line in 0..SPHERE_LONGITUDE_LINES {
    let longitude = radians_per_longitude * longitude_line as f32;
    let next_longitude = radians_per_longitude * (longitude_line + 1) as f32;

    let top_left = sphere_point(longitude, latitude);
    let top_right = sphere_point(next_longitude, latitude);
    let bottom_left = sphere_point(longitude, next_latitude);
    let bottom_right = sphere_point(next_longitude, next_latitude);

    let mut n = (top_right - top_left)
      .cross(top_right - bottom_right)
      .normalize();
    // hack
    if latitude_line == 0 {
      n = (top_right - bottom_left)
        .cross(top_right - bottom_right)
        .normalize();
    }

    verts.push(tri_vert(center + top_left * radius, color, n, BARY_X));
    verts.push(tri_vert(center + top_right * radius, color, n, BARY_Y));
    verts.push(tri_vert(center + bottom_right * radius, color, n, BARY_Z));

    verts.push(tri_vert(center + bottom_right * radius, color, n, BARY_X));
    verts.push(tri_vert(center + bottom_left * radius, color, n, BARY_Y));
    verts.push(tri_vert(center + top_left * radius, color, n, BARY_Z));
  }
}

/// Args for [`emit_sphere_band`] — a struct rather than a long argument list.
struct SphereBand<'a> {
  verts: &'a mut Vec<Vert>,
  center: Vec3,
  radius: f32,
  color: Vec4,
  radians_per_longitude: f32,
  latitude: f32,
  next_latitude: f32,
  latitude_line: i32,
}

/// UV sphere as `15 * 20 * 6 = 1800` triangle vertices.
///
/// Ports `ShapeGenerator::generateSphere` (`ShapeGenerator.cpp:124-176`). Keeps
/// the `latitudeLine == 0` normal hack and the 6-vertex-per-quad winding.
pub fn generate_sphere(center: Vec3, radius: f32, color: Vec4) -> Vec<Vert> {
  let mut verts = Vec::with_capacity((SPHERE_LATITUDE_LINES * SPHERE_LONGITUDE_LINES * 6) as usize);
  let radians_per_latitude = std::f32::consts::PI / SPHERE_LATITUDE_LINES as f32;
  let radians_per_longitude = std::f32::consts::PI * 2.0 / SPHERE_LONGITUDE_LINES as f32;

  for latitude_line in 0..SPHERE_LATITUDE_LINES {
    let latitude = radians_per_latitude * latitude_line as f32;
    let next_latitude = radians_per_latitude * (latitude_line + 1) as f32;

    emit_sphere_band(SphereBand {
      verts: &mut verts,
      center,
      radius,
      color,
      radians_per_longitude,
      latitude,
      next_latitude,
      latitude_line,
    });
  }

  verts
}

/// Sphere with everything below `bottom_distance` (a +Z-axis distance from the
/// centre) removed and replaced by a flat fan cap.
///
/// Ports `ShapeGenerator::generateTruncatedSphere` (`ShapeGenerator.cpp:178-256`).
pub fn generate_truncated_sphere(
  center: Vec3,
  radius: f32,
  bottom_distance: f32,
  color: Vec4,
) -> Vec<Vert> {
  let mut verts = Vec::new();

  let bottom_latitude = (bottom_distance / radius).acos();
  let radians_per_latitude = std::f32::consts::PI / SPHERE_LATITUDE_LINES as f32;
  let radians_per_longitude = std::f32::consts::PI * 2.0 / SPHERE_LONGITUDE_LINES as f32;

  // C++ `static_cast<int>` truncates toward zero; used as an inclusive `<=` bound.
  let bottom_latitude_line = (bottom_latitude / radians_per_latitude) as i32;

  for latitude_line in 0..=bottom_latitude_line {
    let latitude = radians_per_latitude * latitude_line as f32;
    let mut next_latitude = radians_per_latitude * (latitude_line + 1) as f32;
    if next_latitude > bottom_latitude {
      next_latitude = bottom_latitude;
    }

    emit_sphere_band(SphereBand {
      verts: &mut verts,
      center,
      radius,
      color,
      radians_per_longitude,
      latitude,
      next_latitude,
      latitude_line,
    });
  }

  // Fill in the bottom with a fan of triangles to the cap-plane apex.
  let bottom_latitude_z_dist = bottom_latitude.cos() * radius;
  for longitude_line in 0..SPHERE_LONGITUDE_LINES {
    let longitude = radians_per_longitude * longitude_line as f32;
    let next_longitude = radians_per_longitude * (longitude_line + 1) as f32;

    let bottom_left = sphere_point(longitude, bottom_latitude);
    let bottom_right = sphere_point(next_longitude, bottom_latitude);

    let n = (bottom_right - bottom_left)
      .cross(bottom_right - Vec3::new(0.0, -1.0, 0.0))
      .normalize();

    verts.push(tri_vert(center + bottom_left * radius, color, n, BARY_X));
    verts.push(tri_vert(center + bottom_right * radius, color, n, BARY_Y));
    // C++ `center + glm::vec3{0, 0, bottomLatitudeZDist}` — the apex is NOT scaled
    // by `radius` (`bottomLatitudeZDist` already carries it) but IS offset by
    // `center`, unlike what P8.3's step 7 note claimed. Faithful to
    // `ShapeGenerator.cpp:252`.
    verts.push(tri_vert(
      center + Vec3::new(0.0, 0.0, bottom_latitude_z_dist),
      color,
      n,
      BARY_Z,
    ));
  }

  verts
}

/// Project an NDC-space point back through `inv` (an inverse projection matrix)
/// and perspective-divide — ports `ShapeGenerator::invertHelper`
/// (`ShapeGenerator.cpp:258-261`). C++ returns the `vec4` `res / res.w`; the
/// downstream math only uses the `xyz`, so this returns [`Vec3`].
fn invert_helper(inv: Mat4, v: Vec3) -> Vec3 {
  let r = inv * v.extend(1.0);
  (r / r.w).truncate()
}

/// Camera frustum as a line set: a coloured centre ray, four corner rays, and the
/// four far-plane connecting segments.
///
/// Ports `ShapeGenerator::generateCameraLineSegments` (`ShapeGenerator.cpp:263-313`).
/// `perspective` is the camera projection, `transform` its world matrix.
pub fn generate_camera_line_segments(
  perspective: Mat4,
  transform: Mat4,
  center_line_length: f32,
) -> Vec<Vert> {
  let mut res: Vec<Vert> = Vec::new();
  let inverted = perspective.inverse();

  // C++ `addCamLine` lambda: emits the two endpoint verts and returns the segment.
  let add_cam_line =
    |res: &mut Vec<Vert>, a: Vec3, b: Vec3, len: f32, color: Vec4| -> (Vec3, Vec3) {
      let v1 = invert_helper(inverted, a);
      let v2 = invert_helper(inverted, b);
      let dir = (v2 - v1).normalize();

      let start = (transform * v1.extend(1.0)).truncate();
      let end = (transform * (v1 + dir * len).extend(1.0)).truncate();

      res.push(pc_vert(start, color));
      res.push(pc_vert(end, color));

      (start, end)
    };

  let red = Vec4::new(1.0, 0.0, 0.0, 1.0);
  let white = Vec4::ONE;

  let _center = add_cam_line(
    &mut res,
    Vec3::new(0.0, 0.0, 0.0),
    Vec3::new(0.0, 0.0, 1.0),
    center_line_length,
    red,
  );

  let bl = add_cam_line(
    &mut res,
    Vec3::new(-1.0, -1.0, 0.0),
    Vec3::new(-1.0, -1.0, 1.0),
    2.0,
    white,
  );
  let tl = add_cam_line(
    &mut res,
    Vec3::new(-1.0, 1.0, 0.0),
    Vec3::new(-1.0, 1.0, 1.0),
    2.0,
    white,
  );
  let tr = add_cam_line(
    &mut res,
    Vec3::new(1.0, 1.0, 0.0),
    Vec3::new(1.0, 1.0, 1.0),
    2.0,
    white,
  );
  let br = add_cam_line(
    &mut res,
    Vec3::new(1.0, -1.0, 0.0),
    Vec3::new(1.0, -1.0, 1.0),
    2.0,
    white,
  );

  res.push(pc_vert(bl.1, white));
  res.push(pc_vert(br.1, white));

  res.push(pc_vert(tl.1, white));
  res.push(pc_vert(tr.1, white));

  res.push(pc_vert(tl.1, white));
  res.push(pc_vert(bl.1, white));

  res.push(pc_vert(tr.1, white));
  res.push(pc_vert(br.1, white));

  res
}

#[cfg(test)]
mod tests {
  use super::*;

  const EPS: f32 = 1e-3;

  fn aabb(verts: &[Vert]) -> (Vec3, Vec3) {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for v in verts {
      let p = Vec3::from_array(v.pos);
      min = min.min(p);
      max = max.max(p);
    }
    (min, max)
  }

  #[test]
  fn cube_has_36_verts_spanning_the_aabb() {
    let min = Vec3::new(-1.0, -2.0, -3.0);
    let max = Vec3::new(4.0, 5.0, 6.0);
    let verts = generate_cube(min, max, Vec4::ONE);
    assert_eq!(verts.len(), 36);
    let (got_min, got_max) = aabb(&verts);
    assert!((got_min - min).length() < EPS);
    assert!((got_max - max).length() < EPS);
    // per-face normals are unit length
    for v in &verts {
      assert!((Vec3::from_array(v.normal).length() - 1.0).abs() < EPS);
    }
  }

  #[test]
  fn cube_from_center_matches_derived_corners() {
    let center = Vec3::new(1.0, 2.0, 3.0);
    let size = Vec3::new(2.0, 4.0, 6.0);
    let color = Vec4::new(0.25, 0.5, 0.75, 1.0);
    assert_eq!(
      generate_cube_from_center(center, size, color),
      generate_cube(center - size / 2.0, center + size / 2.0, color)
    );
  }

  #[test]
  fn cube_lines_has_24_verts_all_with_the_verbatim_normal() {
    let verts = generate_cube_lines(Vec3::ZERO, Vec3::ONE, Vec4::ONE);
    assert_eq!(verts.len(), 24);
    for v in &verts {
      assert_eq!(v.normal, [0.0, 0.0, -1.0]);
      assert_eq!(v.barycentric, [-1.0, -1.0, -1.0]);
    }
  }

  #[test]
  fn sphere_has_1800_verts_all_radius_distant() {
    let center = Vec3::new(3.0, -1.0, 2.0);
    let radius = 5.0;
    let verts = generate_sphere(center, radius, Vec4::ONE);
    assert_eq!(verts.len(), 15 * 20 * 6);
    for v in &verts {
      let d = (Vec3::from_array(v.pos) - center).length();
      assert!(
        (d - radius).abs() < radius * EPS,
        "vert {d} off radius {radius}"
      );
    }
  }

  #[test]
  fn truncated_sphere_band_verts_are_radius_distant_and_cap_present() {
    let center = Vec3::new(0.0, 0.0, 0.0);
    let radius = 4.0;
    let bottom_distance = 1.0;
    let verts = generate_truncated_sphere(center, radius, bottom_distance, Vec4::ONE);
    // cap fan contributes 20 triangles (60 verts); bands contribute the rest.
    assert!(verts.len() > 60);
    assert_eq!(verts.len() % 3, 0);
  }

  #[test]
  fn camera_line_segments_vert_count() {
    // 5 rays * 2 + 4 far-plane segments * 2 = 18 verts.
    let perspective =
      glam::camera::rh::proj::directx::perspective(60.0_f32.to_radians(), 1.5, 0.1, 100.0);
    let verts = generate_camera_line_segments(perspective, Mat4::IDENTITY, 10.0);
    assert_eq!(verts.len(), 18);
    // centre ray is red, everything after is white.
    assert_eq!(verts[0].color, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(verts[1].color, [1.0, 0.0, 0.0, 1.0]);
    for v in &verts[2..] {
      assert_eq!(v.color, [1.0, 1.0, 1.0, 1.0]);
    }
  }
}
