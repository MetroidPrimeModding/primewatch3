//! The "Objects" window (count, vtable aggregation, "List of types" table,
//! filter + entity table) plus the per-`WatchedEditorId` watch-window loop.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::ctx::Ctx;
use crate::inspector::Inspector;
use crate::mem::game_object_utils::TUniqueID;
use crate::mem::vtables::vtable_class_name;
use crate::object_filter::ObjectFilter;
use crate::structs::prime_structs::GameInstance;

/// One entry in the "watch this editor ID" list.
/// Clicking a row in the "Objects" entity table upserts one of these; each
/// drives its own egui window and contributes its `last_known_uid` to the world
/// highlight set.
pub(super) struct WatchedEditorId {
  pub(super) eid: u32,
  pub(super) last_known_uid: u16,
  pub(super) type_name: String,
}

/// All state mutated here is local UI state (no memory writes), so it mutates
/// the passed `&mut` refs directly rather than deferring like `MenuAction`.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_objects_window(
  egui_ctx: &egui::Context,
  ctx: &Ctx,
  inspector: &Inspector,
  objects: &BTreeMap<TUniqueID, GameInstance>,
  editor_ids_to_watch: &mut Vec<WatchedEditorId>,
  show_active_in_table_only: &mut bool,
  table_hovered_uid: &mut u16,
  object_filter: &mut ObjectFilter,
  unknown_vtables: &mut BTreeSet<u32>,
) {
  // Build the lookup maps and vtable histogram from the live object list.
  let mut vtables: BTreeMap<u32, (u32, u32)> = BTreeMap::new();
  let mut eid_to_entity: HashMap<u32, &GameInstance> = HashMap::new();
  let mut uid_to_entity: HashMap<u16, &GameInstance> = HashMap::new();
  for entity in objects.values() {
    let vtable = entity.member(ctx, "vtable").read_u32(ctx).unwrap_or(0);
    let active = entity.member(ctx, "active").read_bool(ctx).unwrap_or(false);
    let slot = vtables.entry(vtable).or_insert((0, 0));
    if active {
      slot.0 += 1;
    } else {
      slot.1 += 1;
    }
    let eid = entity.member(ctx, "editorID").read_u32(ctx).unwrap_or(0);
    let uid = entity.member(ctx, "uniqueID").read_u16(ctx).unwrap_or(0);
    eid_to_entity.insert(eid, entity);
    uid_to_entity.insert(uid, entity);
  }

  // Accumulate never-before-seen vtable addresses. The
  // `> 0x80000000 && < 0x80700000` window skips the "not up to date yet"
  // sub-0x80000000 garbage.
  for &vtable in vtables.keys() {
    if vtable_class_name(vtable).is_none() && vtable > 0x8000_0000 && vtable < 0x8070_0000 {
      unknown_vtables.insert(vtable);
    }
  }

  // `objects` is now a `BTreeMap`, so `iter()` is already sorted by `TUniqueID`.
  let ordered: Vec<(&TUniqueID, &GameInstance)> = objects.iter().collect();

  egui::Window::new("Objects").show(egui_ctx, |ui| {
    ui.label(format!("Current object count: {}", objects.len()));

    // "Copy unknowns (N)".
    if ui
      .button(format!("Copy unknowns ({})", unknown_vtables.len()))
      .clicked()
    {
      let mut clip = String::new();
      for vt in unknown_vtables.iter() {
        clip.push_str(&format!("{{0x{vt:08x}, \"\"}},\n"));
      }
      ui.ctx().copy_text(clip);
    }

    // "List of types" 4-col table.
    egui::CollapsingHeader::new("List of types").show(ui, |ui| {
      egui::Grid::new("objects_vtables")
        .striped(true)
        .show(ui, |ui| {
          ui.label("address");
          ui.label("name");
          ui.label("active");
          ui.label("inactive");
          ui.end_row();
          for (&vtable, &(active, inactive)) in &vtables {
            if ui
              .selectable_label(false, format!("{vtable:08x}"))
              .clicked()
            {
              ui.ctx().copy_text(format!("{{0x{vtable:08x}, \"\"}},"));
            }
            ui.label(vtable_class_name(vtable).unwrap_or("unknown"));
            ui.label(active.to_string());
            ui.label(inactive.to_string());
            ui.end_row();
          }
        });
    });

    // Filter hint, filter box, "show active only".
    ui.label("Editor ID: #38 Class: @CPlayer name: &name");
    ui.label("(or just type what you're looking for)");
    object_filter.ui(ui);
    ui.checkbox(show_active_in_table_only, "Show active only");

    // Reset before the table; row hover sets it.
    *table_hovered_uid = 0xFFFF;

    // The 5-col scrolling entity table.
    egui::ScrollArea::vertical()
      .max_height(400.0)
      .auto_shrink([false, false])
      .show(ui, |ui| {
        egui::Grid::new("objects_entities")
          .striped(true)
          .show(ui, |ui| {
            ui.label("class");
            ui.label("editorID");
            ui.label("uniqueID");
            ui.label("active");
            ui.label("name");
            ui.end_row();

            for (_, entity) in &ordered {
              let active = entity.member(ctx, "active").read_bool(ctx).unwrap_or(false);
              if *show_active_in_table_only && !active {
                continue;
              }
              let uid = entity.member(ctx, "uniqueID").read_u16(ctx).unwrap_or(0);
              let eid = entity.member(ctx, "editorID").read_u32(ctx).unwrap_or(0);
              let name = entity
                .member(ctx, "name")
                .read_string(ctx)
                .unwrap_or_default();

              // Probe string; sigils `#`/`@`/`&` let a user filter by editor ID
              // / class / name. First `{:08x}` is hex eid, second `{:08}` is
              // decimal eid zero-padded.
              let probe = format!("#{eid:08x}#{eid:08}@{}&{}", entity.type_name, name);
              if !object_filter.passes(&probe) {
                continue;
              }

              let resp = ui.selectable_label(false, entity.type_name.as_ref());
              if resp.clicked() {
                if let Some(watch) = editor_ids_to_watch.iter_mut().find(|w| w.eid == eid) {
                  watch.last_known_uid = uid;
                  watch.type_name = entity.type_name.to_string();
                } else {
                  editor_ids_to_watch.push(WatchedEditorId {
                    eid,
                    last_known_uid: uid,
                    type_name: entity.type_name.to_string(),
                  });
                }
              }
              if resp.hovered() {
                *table_hovered_uid = uid;
              }

              ui.label(format!("{eid:08x}"));
              ui.label(format!("{uid:04x}"));
              ui.label(if active { "yes" } else { "no" });
              ui.label(name);
              ui.end_row();
            }
          });
      });

    ui.label(format!("tableHoveredUid: {}", *table_hovered_uid));
  });

  // One window per watched editor ID. Index-based loop so a window closing
  // (removing its entry) can't skip or panic on the next.
  let mut i = 0;
  while i < editor_ids_to_watch.len() {
    let (eid, last_known_uid, type_name) = {
      let w = &editor_ids_to_watch[i];
      (w.eid, w.last_known_uid, w.type_name.clone())
    };
    let title = format!("{type_name} {eid:08x}");
    let mut open = true;
    let mut new_last_known: Option<u16> = None;

    egui::Window::new(&title)
      .open(&mut open)
      .id(egui::Id::new(("watch", eid)))
      .min_size([240.0, 200.0])
      .show(egui_ctx, |ui| {
        egui::ScrollArea::vertical()
          .auto_shrink([false, true])
          .show(ui, |ui| {
            // Resolve by last-known uid, then by editor ID, then give up.
            let mut handled = false;
            if let Some(entity) = uid_to_entity.get(&last_known_uid) {
              let e_eid = entity.member(ctx, "editorID").read_u32(ctx).unwrap_or(0);
              if e_eid == eid && entity.type_name.as_ref() == type_name {
                inspector.render(ui, ctx, &type_name, entity, false);
                handled = true;
              }
            }
            if !handled && let Some(entity) = eid_to_entity.get(&eid) {
              let uid = entity.member(ctx, "uniqueID").read_u16(ctx).unwrap_or(0);
              new_last_known = Some(uid);
              inspector.render(ui, ctx, &type_name, entity, false);
              handled = true;
            }
            if !handled {
              ui.label("Not loaded");
            }
          });
      });

    if let Some(uid) = new_last_known {
      editor_ids_to_watch[i].last_known_uid = uid;
    }
    if open {
      i += 1;
    } else {
      editor_ids_to_watch.remove(i);
    }
  }
}
