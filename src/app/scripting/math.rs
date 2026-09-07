//! glam `Vec3` / `Quat` / `Mat4` in the scripting engine.
//!
//! Registers the three types as opaque rhai values with constructors, the usual
//! arithmetic operators, a handful of common methods, and the `GameInstance`
//! typed reads (`read_vec3` / `read_quat` / `read_mat4` / `read_transform`),
//! which unpack the game's `CVector3f` / `CQuaternion` / `CMatrix4f` /
//! `CTransform` structs via [`crate::mem::math_utils`].
//!
//! Numeric constructor / setter arguments accept either a rhai INT or FLOAT
//! (`vec3(0, 1, 2)` and `vec3(0.0, 1.0, 2.0)` both work); scalar operators are
//! registered for both `INT` and `FLOAT` right-hand sides.

use glam::{Mat4, Quat, Vec3};
use rhai::{Dynamic, Engine, EvalAltResult};

use crate::mem::math_utils::{read_as_matrix4f, read_as_quat, read_as_transform, read_as_vec3};
use crate::structs::prime_structs::GameInstance;

use super::env::with_env;

/// Coerce a rhai numeric `Dynamic` (INT or FLOAT) to `f32`; `0.0` for anything
/// else so a stray argument type can't wedge a script.
fn num(d: Dynamic) -> f32 {
  if d.is_float() {
    d.as_float().unwrap() as f32
  } else if d.is_int() {
    d.as_int().unwrap() as f32
  } else {
    0.0
  }
}

/// `Some(v)` -> a `Dynamic` of the glam value, `None` -> unit, matching the
/// `read_f32` / `read_u32` / ... convention in [`super::engine`].
fn opt<T: Clone + Send + Sync + 'static>(v: Option<T>) -> Dynamic {
  v.map(Dynamic::from).unwrap_or(Dynamic::UNIT)
}

pub(super) fn register_math_api(engine: &mut Engine) {
  engine
    .register_type_with_name::<Vec3>("Vec3")
    .register_type_with_name::<Quat>("Quat")
    .register_type_with_name::<Mat4>("Mat4");

  register_vec3(engine);
  register_quat(engine);
  register_mat4(engine);
  register_reads(engine);
}

fn register_vec3(engine: &mut Engine) {
  engine.register_fn("vec3", || Vec3::ZERO);
  engine.register_fn("vec3", |x: Dynamic, y: Dynamic, z: Dynamic| {
    Vec3::new(num(x), num(y), num(z))
  });
  engine.register_fn("vec3_splat", |v: Dynamic| Vec3::splat(num(v)));

  engine.register_get_set(
    "x",
    |v: &mut Vec3| v.x as f64,
    |v: &mut Vec3, n: f64| v.x = n as f32,
  );
  engine.register_get_set(
    "y",
    |v: &mut Vec3| v.y as f64,
    |v: &mut Vec3, n: f64| v.y = n as f32,
  );
  engine.register_get_set(
    "z",
    |v: &mut Vec3| v.z as f64,
    |v: &mut Vec3, n: f64| v.z = n as f32,
  );

  engine.register_fn("+", |a: Vec3, b: Vec3| a + b);
  engine.register_fn("-", |a: Vec3, b: Vec3| a - b);
  engine.register_fn("*", |a: Vec3, b: Vec3| a * b);
  engine.register_fn("/", |a: Vec3, b: Vec3| a / b);
  engine.register_fn("-", |a: Vec3| -a);
  engine.register_fn("*", |a: Vec3, s: f64| a * s as f32);
  engine.register_fn("*", |s: f64, a: Vec3| a * s as f32);
  engine.register_fn("*", |a: Vec3, s: i64| a * s as f32);
  engine.register_fn("*", |s: i64, a: Vec3| a * s as f32);
  engine.register_fn("/", |a: Vec3, s: f64| a / s as f32);
  engine.register_fn("/", |a: Vec3, s: i64| a / s as f32);
  engine.register_fn("==", |a: Vec3, b: Vec3| a == b);
  engine.register_fn("!=", |a: Vec3, b: Vec3| a != b);

  engine.register_fn("length", |v: &mut Vec3| v.length() as f64);
  engine.register_fn("length_squared", |v: &mut Vec3| v.length_squared() as f64);
  engine.register_fn("normalize", |v: &mut Vec3| v.normalize_or_zero());
  engine.register_fn("dot", |a: &mut Vec3, b: Vec3| a.dot(b) as f64);
  engine.register_fn("cross", |a: &mut Vec3, b: Vec3| a.cross(b));
  engine.register_fn("distance", |a: &mut Vec3, b: Vec3| a.distance(b) as f64);
  engine.register_fn("angle_between", |a: &mut Vec3, b: Vec3| {
    a.angle_between(b) as f64
  });
  engine.register_fn("lerp", |a: &mut Vec3, b: Vec3, t: f64| a.lerp(b, t as f32));
  engine.register_fn("to_string", |v: &mut Vec3| {
    format!("({}, {}, {})", v.x, v.y, v.z)
  });
  engine.register_fn("to_debug", |v: &mut Vec3| format!("{v:?}"));
}

fn register_quat(engine: &mut Engine) {
  engine.register_fn("quat", || Quat::IDENTITY);
  engine.register_fn("quat_identity", || Quat::IDENTITY);
  engine.register_fn("quat", |x: Dynamic, y: Dynamic, z: Dynamic, w: Dynamic| {
    Quat::from_xyzw(num(x), num(y), num(z), num(w))
  });
  engine.register_fn("quat_from_axis_angle", |axis: Vec3, angle: f64| {
    Quat::from_axis_angle(axis.normalize_or_zero(), angle as f32)
  });

  engine.register_get("x", |q: &mut Quat| q.x as f64);
  engine.register_get("y", |q: &mut Quat| q.y as f64);
  engine.register_get("z", |q: &mut Quat| q.z as f64);
  engine.register_get("w", |q: &mut Quat| q.w as f64);

  engine.register_fn("*", |a: Quat, b: Quat| a * b);
  engine.register_fn("*", |q: Quat, v: Vec3| q * v);
  engine.register_fn("-", |q: Quat| -q);
  engine.register_fn("==", |a: Quat, b: Quat| a == b);
  engine.register_fn("!=", |a: Quat, b: Quat| a != b);

  engine.register_fn("normalize", |q: &mut Quat| q.normalize());
  engine.register_fn("inverse", |q: &mut Quat| q.inverse());
  engine.register_fn("conjugate", |q: &mut Quat| q.conjugate());
  engine.register_fn("length", |q: &mut Quat| q.length() as f64);
  engine.register_fn("dot", |a: &mut Quat, b: Quat| a.dot(b) as f64);
  engine.register_fn("slerp", |a: &mut Quat, b: Quat, t: f64| {
    a.slerp(b, t as f32)
  });
  engine.register_fn("rotate_vec3", |q: &mut Quat, v: Vec3| *q * v);
  engine.register_fn("to_string", |q: &mut Quat| {
    format!("({}, {}, {}, {})", q.x, q.y, q.z, q.w)
  });
  engine.register_fn("to_debug", |q: &mut Quat| format!("{q:?}"));
}

fn register_mat4(engine: &mut Engine) {
  engine.register_fn("mat4", || Mat4::IDENTITY);
  engine.register_fn("mat4_identity", || Mat4::IDENTITY);
  engine.register_fn("mat4_from_translation", Mat4::from_translation);
  engine.register_fn("mat4_from_rotation", Mat4::from_quat);
  engine.register_fn("mat4_from_scale", Mat4::from_scale);
  engine.register_fn(
    "mat4_from_scale_rotation_translation",
    Mat4::from_scale_rotation_translation,
  );

  engine.register_fn("*", |a: Mat4, b: Mat4| a * b);
  engine.register_fn("*", |m: Mat4, v: Vec3| m.transform_point3(v));
  engine.register_fn("==", |a: Mat4, b: Mat4| a == b);
  engine.register_fn("!=", |a: Mat4, b: Mat4| a != b);

  engine.register_fn("inverse", |m: &mut Mat4| m.inverse());
  engine.register_fn("transpose", |m: &mut Mat4| m.transpose());
  engine.register_fn("determinant", |m: &mut Mat4| m.determinant() as f64);
  engine.register_fn("transform_point3", |m: &mut Mat4, v: Vec3| {
    m.transform_point3(v)
  });
  engine.register_fn("transform_vector3", |m: &mut Mat4, v: Vec3| {
    m.transform_vector3(v)
  });
  engine.register_fn("translation", |m: &mut Mat4| m.w_axis.truncate());
  engine.register_fn("rotation", |m: &mut Mat4| {
    m.to_scale_rotation_translation().1
  });
  engine.register_fn("scale", |m: &mut Mat4| m.to_scale_rotation_translation().0);
  engine.register_fn("to_string", |m: &mut Mat4| m.to_string());
  engine.register_fn("to_debug", |m: &mut Mat4| format!("{m:?}"));
}

fn register_reads(engine: &mut Engine) {
  engine.register_fn(
    "read_vec3",
    |inst: &mut GameInstance| -> Result<Dynamic, Box<EvalAltResult>> {
      with_env(|ctx, _| opt(read_as_vec3(ctx, inst)))
    },
  );
  engine.register_fn(
    "read_quat",
    |inst: &mut GameInstance| -> Result<Dynamic, Box<EvalAltResult>> {
      with_env(|ctx, _| opt(read_as_quat(ctx, inst)))
    },
  );
  engine.register_fn(
    "read_mat4",
    |inst: &mut GameInstance| -> Result<Dynamic, Box<EvalAltResult>> {
      with_env(|ctx, _| opt(read_as_matrix4f(ctx, inst)))
    },
  );
  engine.register_fn(
    "read_transform",
    |inst: &mut GameInstance| -> Result<Dynamic, Box<EvalAltResult>> {
      with_env(|ctx, _| opt(read_as_transform(ctx, inst)))
    },
  );
}
