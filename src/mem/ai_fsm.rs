//! Reads a `CPatterned`'s AI state machine (`CStateMachine`) out of live memory.
//!
//! The graph comes from per-enemy `AFSM` assets, but the loaded form is plain data:
//! `CAiState`s whose `mFirstTrigger` points at `mNumTriggers` chain heads, each head
//! linked to further AND-ed conditions through `mAndTrigger` (evaluated head to
//! tail; the *tail* carries the target `mState`). State names are stored
//! inline; trigger names are not, so they are recovered by matching each trigger's
//! `mFunc` (a 12-byte CodeWarrior pointer-to-member) against `CAi::mFuncMap`.

use std::collections::HashMap;

use crate::ctx::Ctx;
use crate::structs::prime_structs::GameInstance;

const MAX_STATES: u32 = 256;
const MAX_FUNC_ENTRIES: u32 = 512;
const MAX_CHAIN_DEPTH: usize = 16;

/// Raw words of a pointer-to-member function. Compared bytewise: virtual and
/// non-virtual members both round-trip identically through `CAiFuncMap`.
type Ptmf = [u32; 3];

#[derive(Clone, Debug, PartialEq)]
pub struct FsmCond {
  pub name: String,
  pub arg: f32,
  pub negated: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FsmChain {
  pub conds: Vec<FsmCond>,
  pub target: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FsmState {
  pub name: String,
  pub chains: Vec<FsmChain>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FsmGraph {
  /// Address of the `CStateMachine`, shared by every enemy using the same `AFSM`.
  pub machine: u32,
  pub states: Vec<FsmState>,
  pub current: Option<usize>,
}

fn is_game_ptr(addr: u32) -> bool {
  addr >= 0x8000_0000
}

fn read_ptmf(ctx: &Ctx, addr: u32) -> Option<Ptmf> {
  Some([
    ctx.mem.read_u32(addr)?,
    ctx.mem.read_u32(addr.wrapping_add(4))?,
    ctx.mem.read_u32(addr.wrapping_add(8))?,
  ])
}

fn trigger_names(ctx: &Ctx) -> HashMap<Ptmf, String> {
  let mut names = HashMap::new();
  let Some(map_ptr) = ctx
    .symbol("mFuncMap__3CAi")
    .and_then(|a| ctx.mem.read_u32(a))
    .filter(|&p| is_game_ptr(p))
  else {
    return names;
  };
  let map = GameInstance::new(map_ptr, "CAiFuncMap".to_string());
  let Some(triggers) = map.get_member(ctx, "triggers") else {
    return names;
  };
  let count = triggers
    .get_member(ctx, "count")
    .and_then(|c| c.read_u32(ctx))
    .unwrap_or(0)
    .min(MAX_FUNC_ENTRIES);
  let Some(first) = triggers.get_member(ctx, "first") else {
    return names;
  };
  for i in 0..count {
    let entry = first.element(ctx, i);
    let Some(name) = entry
      .get_member(ctx, "name")
      .and_then(|n| n.read_string(ctx))
    else {
      continue;
    };
    if let Some(func) = read_ptmf(ctx, entry.address.wrapping_add(4)) {
      names.insert(func, name);
    }
  }
  names
}

fn trigger_label(names: &HashMap<Ptmf, String>, func: Ptmf) -> String {
  if func == [0; 3] {
    // `CAiTrigger::CallFunc` treats a null `mFunc` as always true.
    return "Always".to_string();
  }
  names
    .get(&func)
    .cloned()
    .unwrap_or_else(|| format!("?{:x}:{:x}:{:x}", func[0], func[1], func[2]))
}

/// `None` if `entity` has no loaded state machine (not a `CPatterned`, or unreadable).
pub fn read_fsm(ctx: &Ctx, entity: &GameInstance) -> Option<FsmGraph> {
  let sms = entity.get_member(ctx, "stateMachineState")?;
  let machine = sms.get_member(ctx, "machine")?;
  if !is_game_ptr(machine.address) {
    return None;
  }
  let states_vec = machine.get_member(ctx, "states")?;
  let count = states_vec
    .get_member(ctx, "count")?
    .read_u32(ctx)?
    .min(MAX_STATES);
  let first = states_vec.get_member(ctx, "first")?;
  if !is_game_ptr(first.address) {
    return None;
  }
  let stride = first.element_size(ctx).max(1);
  let state_index = |addr: u32| -> Option<usize> {
    let off = addr.checked_sub(first.address)?;
    let idx = off / stride;
    (off % stride == 0 && idx < count).then_some(idx as usize)
  };

  let names = trigger_names(ctx);
  let mut states = Vec::with_capacity(count as usize);
  for i in 0..count {
    let state = first.element(ctx, i);
    let name = state
      .get_member(ctx, "name")
      .and_then(|n| n.read_string(ctx))
      .unwrap_or_default();
    let num = state
      .get_member(ctx, "numTriggers")
      .and_then(|n| n.read_u32(ctx))
      .unwrap_or(0)
      .min(MAX_STATES);
    let mut chains = Vec::new();
    if let Some(heads) = state.get_member(ctx, "firstTrigger")
      && is_game_ptr(heads.address)
    {
      for h in 0..num {
        let head = heads.element(ctx, h);
        let mut target = None;
        let mut conds = Vec::new();
        let mut cur = Some(head);
        while let Some(trig) = cur.take() {
          if conds.len() >= MAX_CHAIN_DEPTH {
            break;
          }
          let func = read_ptmf(ctx, trig.address).unwrap_or([0; 3]);
          conds.push(FsmCond {
            name: trigger_label(&names, func),
            arg: trig
              .get_member(ctx, "arg")
              .and_then(|a| a.read_f32(ctx))
              .unwrap_or(0.0),
            negated: trig
              .get_member(ctx, "negated")
              .and_then(|n| n.read_bool(ctx))
              .unwrap_or(false),
          });
          target = trig
            .get_member(ctx, "state")
            .and_then(|s| state_index(s.address));
          cur = trig
            .get_member(ctx, "andTrigger")
            .filter(|n| is_game_ptr(n.address));
        }
        chains.push(FsmChain { conds, target });
      }
    }
    states.push(FsmState { name, chains });
  }

  let current = sms
    .get_member(ctx, "state")
    .and_then(|s| state_index(s.address));
  Some(FsmGraph {
    machine: machine.address,
    states,
    current,
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::mem::game_memory::GameMemory;
  use crate::mem::game_object_utils::get_all_objects;
  use crate::mem::game_version::GameVersion;
  use crate::structs::prime_structs::GameStructs;

  #[test]
  fn unknown_functions_get_a_stable_placeholder_label() {
    let names = HashMap::new();
    assert_eq!(trigger_label(&names, [0, 0, 0]), "Always");
    assert_eq!(trigger_label(&names, [0, 0x1c, 0x80]), "?0:1c:80");
  }

  #[test]
  fn known_functions_resolve_by_raw_words() {
    let mut names = HashMap::new();
    names.insert([0, 8, 0x8000_1234], "InRange".to_string());
    assert_eq!(trigger_label(&names, [0, 8, 0x8000_1234]), "InRange");
  }

  /// Any `CPatterned` found in the repo's `.raw` dumps must yield a coherent graph:
  /// non-empty, current state (if set) named, every target in range.
  #[test]
  fn graphs_read_from_dumps_are_coherent() {
    let mut structs = GameStructs::new_empty();
    structs
      .load_from_dir(
        concat!(env!("CARGO_MANIFEST_DIR"), "/prime_defs"),
        GameVersion::default(),
      )
      .expect("load prime_defs");
    let mut checked = 0;
    for entry in std::fs::read_dir(env!("CARGO_MANIFEST_DIR"))
      .unwrap()
      .flatten()
    {
      let path = entry.path();
      if path.extension().is_none_or(|e| e != "raw") {
        continue;
      }
      let mut mem = GameMemory::new();
      mem.load_from_file(path.to_str().unwrap()).unwrap();
      let ctx = Ctx::new(&structs, &mem, GameVersion::default());
      for entity in get_all_objects(&ctx).values() {
        if !entity.extends_class(&ctx, "CPatterned") {
          continue;
        }
        let Some(g) = read_fsm(&ctx, entity) else {
          continue;
        };
        eprintln!(
          "{}: {} {} states, current {:?}",
          path.display(),
          entity.type_name,
          g.states.len(),
          g.current.map(|c| &g.states[c].name)
        );
        assert!(!g.states.is_empty());
        assert!(g.states.iter().all(|s| !s.name.is_empty()));
        for s in &g.states {
          for c in &s.chains {
            assert!(!c.conds.is_empty());
            for cond in &c.conds {
              assert!(!cond.name.starts_with('?'), "unresolved trigger {cond:?}");
            }
            assert!(c.target.is_some(), "chain without a target in {}", s.name);
          }
        }
        checked += 1;
      }
    }
    eprintln!("checked {checked} state machines");
  }
}
