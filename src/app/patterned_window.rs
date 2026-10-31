//! "Patterned AI" window: every `CPatterned`-derived object with its current
//! AI state-machine state.

use std::collections::BTreeMap;

use crate::ctx::Ctx;
use crate::mem::game_object_utils::{TUniqueID, patterned_state_name};
use crate::structs::prime_structs::GameInstance;

pub(super) fn render_patterned_window(
  egui_ctx: &egui::Context,
  ctx: &Ctx,
  objects: &BTreeMap<TUniqueID, GameInstance>,
  open: &mut bool,
) {
  egui::Window::new("Patterned AI")
    .open(open)
    .show(egui_ctx, |ui| {
      egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| {
          egui::Grid::new("patterned_states")
            .striped(true)
            .show(ui, |ui| {
              for h in ["uid", "class", "state", "time", "anim", "move", "patrol"] {
                ui.strong(h);
              }
              ui.end_row();
              for (uid, entity) in objects {
                if !entity.extends_class(ctx, "CPatterned") {
                  continue;
                }
                let ms = entity.get_member(ctx, "stateMachineState");
                let time = ms
                  .and_then(|m| m.get_member(ctx, "time"))
                  .and_then(|m| m.read_f32(ctx));
                let enum_cell = |name: &str| {
                  entity
                    .get_member(ctx, name)
                    .and_then(|m| m.read_u32(ctx))
                    .map_or("?".to_string(), |v| (v as i32).to_string())
                };
                ui.label(format!("{uid:#06x}"));
                ui.label(&*entity.type_name);
                ui.label(patterned_state_name(ctx, entity).unwrap_or_else(|| "-".into()));
                ui.label(time.map_or("?".to_string(), |t| format!("{t:.2}")));
                ui.label(enum_cell("animState"));
                ui.label(enum_cell("moveState"));
                ui.label(enum_cell("patrolState"));
                ui.end_row();
              }
            });
        });
    });
}
