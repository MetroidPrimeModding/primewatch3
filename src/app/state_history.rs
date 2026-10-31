//! Remembers the last states each `CPatterned` was seen in, by `CAiState` address
//! (state names repeat within a machine).
//!
//! The game keeps only the current state, so history is built from what the UI
//! observes: a state that lasts less than one UI frame is never seen and will be
//! missing from the history.

use std::collections::{BTreeMap, HashMap};

use crate::ctx::Ctx;
use crate::mem::game_object_utils::{TUniqueID, patterned_state_address};
use crate::structs::prime_structs::GameInstance;

/// How many past states are kept.
pub(super) const HISTORY_LEN: usize = 2;

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct StateHistory {
  current: Option<u32>,
  /// Most recent first.
  previous: Vec<u32>,
}

impl StateHistory {
  /// `None` (no state yet) leaves the history untouched.
  pub(super) fn observe(&mut self, state: Option<u32>) {
    let Some(state) = state else { return };
    if self.current == Some(state) {
      return;
    }
    if let Some(old) = self.current.replace(state) {
      self.previous.insert(0, old);
      self.previous.truncate(HISTORY_LEN);
    }
  }

  /// Most recent first.
  pub(super) fn previous(&self) -> &[u32] {
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
        .observe(patterned_state_address(ctx, entity));
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn seen(h: &mut StateHistory, states: &[u32]) {
    for &s in states {
      h.observe(Some(s));
    }
  }

  #[test]
  fn keeps_the_two_most_recent_previous_states() {
    let mut h = StateHistory::default();
    seen(&mut h, &[1, 2, 3, 4]);
    assert_eq!(h.previous(), [3, 2]);
  }

  #[test]
  fn repeats_and_missing_states_do_not_add_entries() {
    let mut h = StateHistory::default();
    seen(&mut h, &[1, 1, 2, 2]);
    h.observe(None);
    assert_eq!(h.previous(), [1]);
  }

  #[test]
  fn returning_to_an_earlier_state_is_recorded() {
    let mut h = StateHistory::default();
    seen(&mut h, &[10, 20, 10]);
    assert_eq!(h.previous(), [20, 10]);
  }
}
