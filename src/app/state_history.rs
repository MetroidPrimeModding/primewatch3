//! Remembers the last states each `CPatterned` was seen in.
//!
//! The game keeps only the current state, so history is built from what the UI
//! observes: a state that lasts less than one UI frame is never seen and will be
//! missing from the history.

use std::collections::{BTreeMap, HashMap};

use crate::ctx::Ctx;
use crate::mem::game_object_utils::{TUniqueID, patterned_state_name};
use crate::structs::prime_structs::GameInstance;

/// How many past states are kept.
pub(super) const HISTORY_LEN: usize = 2;

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct StateHistory {
  current: Option<String>,
  /// Most recent first.
  previous: Vec<String>,
}

impl StateHistory {
  /// `None` (no state yet) leaves the history untouched.
  pub(super) fn observe(&mut self, state: Option<String>) {
    let Some(state) = state else { return };
    if self.current.as_deref() == Some(state.as_str()) {
      return;
    }
    if let Some(old) = self.current.replace(state) {
      self.previous.insert(0, old);
      self.previous.truncate(HISTORY_LEN);
    }
  }

  /// Most recent first.
  pub(super) fn previous(&self) -> &[String] {
    &self.previous
  }
}

pub(super) type HistoryMap = HashMap<TUniqueID, StateHistory>;

/// Records this frame's state for every `CPatterned` and drops objects that are gone.
pub(super) fn observe_all(
  ctx: &Ctx,
  objects: &BTreeMap<TUniqueID, GameInstance>,
  map: &mut HistoryMap,
) {
  map.retain(|uid, _| objects.contains_key(uid));
  for (uid, entity) in objects {
    if entity.extends_class(ctx, "CPatterned") {
      map
        .entry(*uid)
        .or_default()
        .observe(patterned_state_name(ctx, entity));
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn seen(h: &mut StateHistory, names: &[&str]) {
    for n in names {
      h.observe(Some(n.to_string()));
    }
  }

  #[test]
  fn keeps_the_two_most_recent_previous_states() {
    let mut h = StateHistory::default();
    seen(&mut h, &["Start", "Patrol", "Attack", "Dodge"]);
    assert_eq!(h.previous(), ["Attack", "Patrol"]);
  }

  #[test]
  fn repeats_and_missing_states_do_not_add_entries() {
    let mut h = StateHistory::default();
    seen(&mut h, &["Start", "Start", "Patrol", "Patrol"]);
    h.observe(None);
    assert_eq!(h.previous(), ["Start"]);
  }

  #[test]
  fn returning_to_an_earlier_state_is_recorded() {
    let mut h = StateHistory::default();
    seen(&mut h, &["A", "B", "A"]);
    assert_eq!(h.previous(), ["B", "A"]);
  }
}
