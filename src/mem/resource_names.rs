//! Original asset paths for resource ids, from the `prime-dehashing` submodule's
//! `path -> hash` table (inverted here to `hash -> path`). Release archives ship
//! a copy in `prime_defs/` (see `.github/workflows/rust_build.yml`).

use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ResourceNameDisplay {
  /// Hides the loading monitor entirely.
  Disabled,
  Hash,
  /// Falls back to the hash for ids with no known path.
  #[default]
  Path,
}

const RELEASE_PATH: &str = "prime_defs/resource_names.json";
const SUBMODULE_PATH: &str = "prime-dehashing/output_json/mp_resource_names.json";

#[derive(Debug, Default)]
pub struct ResourceNames {
  by_id: HashMap<u32, Rc<str>>,
}

impl ResourceNames {
  pub fn load(path: &Path) -> Result<ResourceNames, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
  }

  /// Loads the release copy if present, else the submodule's. Missing or
  /// malformed table is non-fatal: ids just show without a path.
  pub fn load_or_empty() -> ResourceNames {
    let path = [RELEASE_PATH, SUBMODULE_PATH]
      .map(Path::new)
      .into_iter()
      .find(|p| p.is_file())
      .unwrap_or(Path::new(SUBMODULE_PATH));
    Self::load(path).unwrap_or_else(|err| {
      eprintln!("Failed to load resource names: {err}");
      ResourceNames::default()
    })
  }

  fn parse(text: &str) -> Result<ResourceNames, String> {
    let by_path: HashMap<String, String> = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let mut by_id = HashMap::with_capacity(by_path.len());
    for (path, hash) in by_path {
      let id =
        u32::from_str_radix(&hash, 16).map_err(|_| format!("bad hash `{hash}` for `{path}`"))?;
      by_id.insert(id, Rc::from(path));
    }
    Ok(ResourceNames { by_id })
  }

  pub fn get(&self, id: u32) -> Option<&str> {
    self.by_id.get(&id).map(|p| &**p)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_inverts_path_to_hash() {
    let names =
      ResourceNames::parse(r#"{"$/Worlds/a.mrea": "7ed253fb", "$/b.txtr": "0000abcd"}"#).unwrap();
    assert_eq!(names.get(0x7ed253fb), Some("$/Worlds/a.mrea"));
    assert_eq!(names.get(0xabcd), Some("$/b.txtr"));
    assert_eq!(names.get(0x1234), None);
  }

  #[test]
  fn parse_rejects_bad_hash() {
    assert!(ResourceNames::parse(r#"{"$/a.txtr": "nothex"}"#).is_err());
  }

  #[test]
  fn submodule_table_loads() {
    let path = Path::new(SUBMODULE_PATH);
    if !path.is_file() {
      return; // submodule not checked out
    }
    let names = ResourceNames::load(path).unwrap();
    assert_eq!(
      names.get(0x7ed253fb),
      Some("$/Ai/StateMachines/babygoth.afsm")
    );
  }
}
