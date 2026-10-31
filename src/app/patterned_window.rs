//! "Patterned AI" window: every `CPatterned`-derived object with its current
//! AI state-machine state, and the selected one's full state machine.

use std::collections::BTreeMap;

use crate::ctx::Ctx;
use crate::mem::ai_fsm::{FsmChain, FsmGraph, read_fsm};
use crate::mem::game_object_utils::{TUniqueID, patterned_state_name};
use crate::structs::prime_structs::GameInstance;

use super::fsm_diagram::{RECENT_COLORS, show_diagram};
use super::state_history::{HISTORY_LEN, HistoryMap, StateHistory, observe_all};

pub(super) fn render_patterned_window(
  egui_ctx: &egui::Context,
  ctx: &Ctx,
  objects: &BTreeMap<TUniqueID, GameInstance>,
  open: &mut bool,
) {
  let history_id = egui::Id::new("patterned-state-history");
  let history: HistoryMap = egui_ctx.data_mut(|d| {
    let map = d.get_temp_mut_or_default::<HistoryMap>(history_id);
    observe_all(ctx, objects, map);
    map.clone()
  });
  let selected_id = egui::Id::new("patterned-selected-uid");
  let mut selected: Option<TUniqueID> = egui_ctx.data(|d| d.get_temp(selected_id));
  let diagram_id = egui::Id::new("patterned-diagram-open");
  let mut diagram_open: bool = egui_ctx.data(|d| d.get_temp(diagram_id)).unwrap_or(false);
  let labels_id = egui::Id::new("patterned-diagram-labels");
  let mut show_labels: bool = egui_ctx.data(|d| d.get_temp(labels_id)).unwrap_or(false);

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
      ui.horizontal(|ui| {
        ui.checkbox(&mut diagram_open, "Diagram window");
        ui.checkbox(&mut show_labels, "Edge labels");
      });
      match read_fsm(ctx, entity) {
        Some(graph) => {
          let recent = recent_states(&graph, selected.and_then(|u| history.get(&u)));
          render_graph(ui, &graph, recent)
        }
        None => {
          ui.label("No state machine loaded for this object.");
        }
      }
    });

  if diagram_open {
    let mut window_open = true;
    egui::Window::new("Patterned FSM diagram")
      .open(&mut window_open)
      .default_size([760.0, 480.0])
      .show(egui_ctx, |ui| {
        match selected.and_then(|uid| objects.get(&uid)) {
          None => {
            ui.label("Select a patterned object in the Patterned AI window.");
          }
          Some(entity) => match read_fsm(ctx, entity) {
            Some(graph) => {
              ui.label(format!(
                "{} {:#06x}",
                entity.type_name,
                selected.unwrap_or(0)
              ));
              let recent = recent_states(&graph, selected.and_then(|u| history.get(&u)));
              show_diagram(ui, &graph, show_labels, recent);
            }
            None => {
              ui.label("No state machine loaded for this object.");
            }
          },
        };
      });
    diagram_open = window_open;
  }

  egui_ctx.data_mut(|d| {
    if let Some(uid) = selected {
      d.insert_temp(selected_id, uid);
    }
    d.insert_temp(diagram_id, diagram_open);
    d.insert_temp(labels_id, show_labels);
  });
}

/// Indices of the previous states in `graph`, most recent first. Matched by name;
/// a state the machine no longer has (different enemy in the same slot) is `None`.
fn recent_states(graph: &FsmGraph, history: Option<&StateHistory>) -> [Option<usize>; HISTORY_LEN] {
  let mut out = [None; HISTORY_LEN];
  for (slot, name) in out
    .iter_mut()
    .zip(history.map_or(&[][..], |h| h.previous()))
  {
    *slot = graph.states.iter().position(|s| &s.name == name);
  }
  out
}

fn render_graph(ui: &mut egui::Ui, graph: &FsmGraph, recent: [Option<usize>; HISTORY_LEN]) {
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
        let recent_rank = recent.iter().position(|r| *r == Some(i));
        let suffix = match recent_rank {
          Some(0) => "  [previous]",
          Some(_) => "  [2 ago]",
          None => "",
        };
        let title = egui::RichText::new(format!("{} ({}){suffix}", state.name, state.chains.len()));
        let title = match recent_rank {
          _ if is_current => title.strong().color(egui::Color32::LIGHT_GREEN),
          Some(rank) => title.color(RECENT_COLORS[rank]),
          None => title,
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

pub(super) fn chain_conds_text(chain: &FsmChain) -> String {
  chain
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
    .join(" & ")
}

pub(super) fn chain_text(graph: &FsmGraph, chain: &FsmChain) -> String {
  let target = chain
    .target
    .and_then(|t| graph.states.get(t))
    .map_or("?", |s| s.name.as_str());
  format!("{}  ->  {target}", chain_conds_text(chain))
}
