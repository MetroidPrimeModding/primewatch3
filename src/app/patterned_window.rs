//! "Patterned AI" window: every `CPatterned`-derived object with its current
//! AI state-machine state, and the selected one's full state machine.

use std::collections::BTreeMap;

use crate::ctx::Ctx;
use crate::mem::ai_fsm::{FsmChain, FsmGraph, read_fsm};
use crate::mem::game_object_utils::{TUniqueID, patterned_state_name};
use crate::structs::prime_structs::GameInstance;

pub(super) fn render_patterned_window(
  egui_ctx: &egui::Context,
  ctx: &Ctx,
  objects: &BTreeMap<TUniqueID, GameInstance>,
  open: &mut bool,
) {
  let selected_id = egui::Id::new("patterned-selected-uid");
  let mut selected: Option<TUniqueID> = egui_ctx.data(|d| d.get_temp(selected_id));

  egui::Window::new("Patterned AI")
    .open(open)
    .default_width(420.0)
    .show(egui_ctx, |ui| {
      egui::ScrollArea::vertical()
        .id_salt("patterned-table")
        .max_height(220.0)
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
                let time = entity
                  .get_member(ctx, "stateMachineState")
                  .and_then(|m| m.get_member(ctx, "time"))
                  .and_then(|m| m.read_f32(ctx));
                let enum_cell = |name: &str| {
                  entity
                    .get_member(ctx, name)
                    .and_then(|m| m.read_u32(ctx))
                    .map_or("?".to_string(), |v| (v as i32).to_string())
                };
                if ui
                  .selectable_label(selected == Some(*uid), format!("{uid:#06x}"))
                  .clicked()
                {
                  selected = Some(*uid);
                }
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

      ui.separator();
      let Some(entity) = selected.and_then(|uid| objects.get(&uid)) else {
        ui.label("Click a uid to show its state machine.");
        return;
      };
      match read_fsm(ctx, entity) {
        Some(graph) => render_graph(ui, &graph),
        None => {
          ui.label("No state machine loaded for this object.");
        }
      }
    });

  if let Some(uid) = selected {
    egui_ctx.data_mut(|d| d.insert_temp(selected_id, uid));
  }
}

fn render_graph(ui: &mut egui::Ui, graph: &FsmGraph) {
  ui.label(format!(
    "State machine @ {:#010x}, {} states",
    graph.machine,
    graph.states.len()
  ));
  egui::ScrollArea::vertical()
    .id_salt("patterned-graph")
    .auto_shrink([false, true])
    .show(ui, |ui| {
      for (i, state) in graph.states.iter().enumerate() {
        let is_current = graph.current == Some(i);
        let title = egui::RichText::new(format!("{} ({})", state.name, state.chains.len()));
        let title = if is_current {
          title.strong().color(egui::Color32::LIGHT_GREEN)
        } else {
          title
        };
        egui::CollapsingHeader::new(title)
          .id_salt(("patterned-state", i))
          .open(is_current.then_some(true))
          .show(ui, |ui| {
            if state.chains.is_empty() {
              ui.weak("no transitions");
            }
            for chain in &state.chains {
              ui.label(chain_text(graph, chain));
            }
          });
      }
    });
}

fn chain_text(graph: &FsmGraph, chain: &FsmChain) -> String {
  let conds = chain
    .conds
    .iter()
    .map(|c| {
      let not = if c.negated { "!" } else { "" };
      if c.arg == 0.0 {
        format!("{not}{}", c.name)
      } else {
        format!("{not}{}({})", c.name, c.arg)
      }
    })
    .collect::<Vec<_>>()
    .join(" & ");
  let target = chain
    .target
    .and_then(|t| graph.states.get(t))
    .map_or("?", |s| s.name.as_str());
  format!("{conds}  ->  {target}")
}
