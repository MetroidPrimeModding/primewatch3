//! The call-scoped environment bridge.
//!
//! See the module docs ([`super`]) for why this thread-local dance is how a
//! borrowed `&Ctx` reaches rhai's `'static`-only registered functions, and why
//! it is sound.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::marker::PhantomData;

use rhai::{EvalAltResult, Position};

use crate::ctx::Ctx;
use crate::mem::game_object_utils::TUniqueID;
use crate::structs::prime_structs::GameInstance;

use super::window::CustomInspectorWindow;

/// Type-erased pointers to the frame's `Ctx` and object map. Reconstituted by
/// [`with_env`]; see the module docs for why this is sound.
#[derive(Clone, Copy)]
struct ScriptEnv {
  ctx: *const (),
  objects: *const (),
}

thread_local! {
  /// `Some` only while a [`ScriptFrame`] is alive on this thread.
  static ENV: Cell<Option<ScriptEnv>> = const { Cell::new(None) };
  /// Windows submitted via `show(w)` during the current script run.
  static SINK: RefCell<Vec<CustomInspectorWindow>> = const { RefCell::new(Vec::new()) };
}

/// Push a window built by a script onto the current frame's sink. Called by the
/// `show` function registered in [`super::engine`].
pub(super) fn submit_window(w: CustomInspectorWindow) {
  SINK.with(|s| s.borrow_mut().push(w));
}

/// RAII guard that publishes `ctx` + `objects` to the thread-local for the
/// duration of one or more `engine.run_ast` calls, then tears them down.
///
/// The lifetime `'a` ties the guard to the borrows it publishes; nothing hands
/// out a `ScriptFrame` that outlives them.
pub(super) struct ScriptFrame<'a> {
  _marker: PhantomData<&'a ()>,
}

impl<'a> ScriptFrame<'a> {
  pub(super) fn enter<'c>(ctx: &'a Ctx<'c>, objects: &'a BTreeMap<TUniqueID, GameInstance>) -> Self
  where
    'c: 'a,
  {
    let env = ScriptEnv {
      // Type/lifetime-erase for storage. `with_env` reconstitutes a borrow that
      // cannot outlive this guard
      ctx: ctx as *const Ctx<'c> as *const (),
      objects: objects as *const BTreeMap<TUniqueID, GameInstance> as *const (),
    };
    ENV.with(|e| e.set(Some(env)));
    SINK.with(|s| s.borrow_mut().clear());
    ScriptFrame {
      _marker: PhantomData,
    }
  }

  pub(super) fn take_windows(&self) -> Vec<CustomInspectorWindow> {
    SINK.with(|s| std::mem::take(&mut *s.borrow_mut()))
  }
}

impl Drop for ScriptFrame<'_> {
  fn drop(&mut self) {
    ENV.with(|e| e.set(None));
    SINK.with(|s| s.borrow_mut().clear());
  }
}

fn no_env() -> Box<EvalAltResult> {
  Box::new(EvalAltResult::ErrorRuntime(
    "primewatch scripting functions can only be called while a script is running".into(),
    Position::NONE,
  ))
}

/// Run `f` against the live `Ctx` + object map published by the enclosing
/// [`ScriptFrame`]. Errors (as a script runtime error) if called outside one.
pub(super) fn with_env<R>(
  f: impl FnOnce(&Ctx, &BTreeMap<TUniqueID, GameInstance>) -> R,
) -> Result<R, Box<EvalAltResult>> {
  ENV.with(|e| {
    let env = e.get().ok_or_else(no_env)?;
    // SAFETY: `ENV` is `Some` only between `ScriptFrame::enter` and its `Drop`.
    // That guard borrows both referents for `'a`, and script execution is
    // synchronous on this thread, so both pointers are live for the call to
    // `f`. `f` returns an owned `R`; no borrow of `*ctx` / `*objects` escapes.
    let ctx = unsafe { &*(env.ctx as *const Ctx) };
    let objects = unsafe { &*(env.objects as *const BTreeMap<TUniqueID, GameInstance>) };
    Ok(f(ctx, objects))
  })
}
