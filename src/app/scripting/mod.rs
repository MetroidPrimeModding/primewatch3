//! Rhai scripting bridge.
//!
//! Scripts live as `*.rhai` files in `./scripts/` and are **re-run every frame**
//! from [`crate::app::App::redraw`], right after the per-frame memory parse and
//! object walk. A script inspects live memory through a small handle API and
//! hands back [`CustomInspectorWindow`]s, which the app draws with the normal
//! `Inspector`.
//!
//! ## The `Ctx` problem
//!
//! Rhai can only register functions whose arguments are `'static` values (plus an
//! optional leading `NativeCallContext`). Our traversal layer needs a
//! `&Ctx<'a>` (borrows `GameStructs` + `GameMemory`), which is neither `'static`
//! nor a value. Registering `fn(&mut GameInstance, &Ctx)` *compiles* but rhai can
//! never supply the second argument, so the function is silently uncallable.
//!
//! So the bridge threads `Ctx` (and the pre-walked object map) through a
//! thread-local set for exactly the duration of one script run by [`env::ScriptFrame`].
//! Every registered function is a capture-free closure that pulls them back out
//! via [`env::with_env`]. This is sound because scripts run **synchronously on one
//! thread** and never retain a handle past the call: the `ScriptFrame` guard
//! borrows both referents and clears the thread-local on drop. It is a transient
//! call-scoped bridge, not the ambient mutable game state the port set out to
//! remove.
//!
//! ## Module layout
//!
//! - [`window`] — the view types a script builds ([`CustomInspectorWindow`] etc.).
//! - [`env`] — the call-scoped `Ctx`/object-map bridge and its [`env::ScriptFrame`] guard.
//! - [`engine`] — [`engine::build_engine`] and the rhai function registrations.
//! - [`math`] — glam `Vec3` / `Quat` / `Mat4` types, operators, and typed reads.
//! - [`manager`] — script discovery, per-frame execution, enabled-state persistence.

mod engine;
mod env;
mod manager;
mod math;
mod window;

#[cfg(test)]
mod tests;

pub use manager::ScriptManager;
pub use window::{AnchorAlign, CustomInspectorRow, CustomInspectorWindow, WindowAnchor};
