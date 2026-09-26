//! Per-revision type layouts exported from a `-g` decomp build by `dtk dwarf types`
//! (`prime_defs/layouts/<VERSION>.json.gz`, see `doc/multi-version.md`).
//!
//! mwcc's DWARF 1.1 names types by bare identifier, so many template instantiations
//! share a name (`vector`, `single_ptr`, ...). dtk lists those under `conflicts`,
//! and every type reference to one carries `variant`, the index of the exact
//! definition it uses. Always resolve through [`LayoutDb::resolve`] rather than
//! looking a bare name up directly.
//!
//! `.bs` members written `= mField` are resolved against these layouts at load time
//! ([`FieldResolver`]).

use crate::mem::game_version::GameVersion;
use bstruct::bstruct_link::{FieldResolver, ResolvedField};
use serde::Deserialize;
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::Read;
use std::sync::OnceLock;

#[derive(Deserialize)]
struct TypeLayoutsFile {
  structs: Vec<StructDef>,
  unions: Vec<StructDef>,
  conflicts: Vec<Conflict>,
}

#[derive(Deserialize)]
struct Conflict {
  name: String,
  category: String,
  variants: Vec<ConflictVariant>,
}

#[derive(Deserialize)]
struct ConflictVariant {
  // Struct or enum depending on the conflict's `category`; only structs are parsed.
  definition: serde_json::Value,
}

#[derive(Deserialize, Clone, Debug)]
pub struct StructDef {
  /// Absent on anonymous (`inline`) definitions.
  #[serde(default)]
  pub name: String,
  pub byte_size: Option<u32>,
  #[serde(default)]
  pub bases: Vec<BaseRef>,
  #[serde(default)]
  pub members: Vec<Member>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct BaseRef {
  pub name: String,
  pub variant: Option<usize>,
  pub offset: u32,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Member {
  pub name: Option<String>,
  /// Relative to the enclosing definition. For bitfields, the storage unit's offset.
  pub offset: u32,
  /// Whole member size (all array elements); for bitfields, the storage unit's size.
  pub byte_size: Option<u32>,
  pub bit: Option<BitData>,
  #[serde(rename = "type")]
  pub ty: TypeRef,
}

/// DWARF bitfield position: `bit_offset` counts from the MSB of the storage unit.
#[derive(Deserialize, Clone, Copy, Debug)]
pub struct BitData {
  pub bit_size: u32,
  pub bit_offset: u32,
}

#[derive(Deserialize, Clone, Debug)]
pub struct TypeRef {
  pub display: String,
  pub base: String,
  /// fundamental | struct | class | union | enum | function | ptr_to_member | unknown
  pub base_kind: String,
  pub variant: Option<usize>,
  /// Innermost first: `const X*` is `["const", "pointer"]`.
  #[serde(default)]
  pub modifiers: Vec<String>,
  /// C order: `short[8][2]` is `[8, 2]`.
  #[serde(default)]
  pub array_dims: Vec<u32>,
  /// Only set for pointer-to-array: the modifiers applied outside the array.
  #[serde(default)]
  pub array_modifiers: Vec<String>,
  /// Anonymous struct/union/enum definition, present when `base` is empty.
  pub inline: Option<serde_json::Value>,
}

fn has_pointer(modifiers: &[String]) -> bool {
  modifiers.iter().any(|m| m == "pointer" || m == "reference")
}

impl TypeRef {
  pub fn is_pointer(&self) -> bool {
    if has_pointer(&self.array_modifiers) {
      return true;
    }
    // An array of pointers is an array; its element (dims stripped) is the pointer.
    // mwcc's vtable pointer is the fundamental `void *` with no modifiers.
    self.array_dims.is_empty() && (has_pointer(&self.modifiers) || self.base.ends_with('*'))
  }

  pub fn is_array(&self) -> bool {
    !self.array_dims.is_empty() && !self.is_pointer()
  }

  pub fn is_aggregate(&self) -> bool {
    !self.is_pointer()
      && self.array_dims.is_empty()
      && matches!(self.base_kind.as_str(), "struct" | "class" | "union")
  }

  /// The element type of an array; `self` unchanged otherwise.
  #[cfg(test)]
  pub fn element(&self) -> TypeRef {
    TypeRef {
      array_dims: Vec::new(),
      ..self.clone()
    }
  }
}

pub struct LayoutDb {
  structs: HashMap<String, StructDef>,
  conflicts: HashMap<String, Vec<Option<StructDef>>>,
}

pub fn short_name(name: &str) -> &str {
  let no_args = name.split('<').next().unwrap_or(name);
  no_args.rsplit("::").next().unwrap_or(no_args)
}

enum PathPart<'a> {
  Field(&'a str),
  Index(u32),
}

/// `a.b[2].c` → `a`, `b`, `[2]`, `c`.
fn parse_path(path: &str) -> Result<Vec<PathPart<'_>>, String> {
  let mut parts = Vec::new();
  for dotted in path.split('.') {
    let (name, mut rest) = dotted.split_at(dotted.find('[').unwrap_or(dotted.len()));
    if name.is_empty() {
      return Err(format!("bad field path `{path}`"));
    }
    parts.push(PathPart::Field(name));
    while let Some(tail) = rest.strip_prefix('[') {
      let (index, tail) = tail
        .split_once(']')
        .ok_or_else(|| format!("bad field path `{path}`"))?;
      parts.push(PathPart::Index(
        index
          .parse()
          .map_err(|_| format!("bad index in `{path}`"))?,
      ));
      rest = tail;
    }
    if !rest.is_empty() {
      return Err(format!("bad field path `{path}`"));
    }
  }
  Ok(parts)
}

impl LayoutDb {
  pub fn from_gz(gz: &[u8]) -> Result<Self, String> {
    let mut json = String::new();
    flate2::read::GzDecoder::new(gz)
      .read_to_string(&mut json)
      .map_err(|e| e.to_string())?;
    Self::from_json(&json).map_err(|e| e.to_string())
  }

  pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
    let file: TypeLayoutsFile = serde_json::from_str(json)?;
    let structs = file
      .structs
      .into_iter()
      .chain(file.unions)
      .map(|s| (s.name.clone(), s))
      .collect();
    let mut conflicts = HashMap::new();
    for c in file.conflicts {
      // Keep enum conflicts as `None` placeholders so variant indices stay aligned.
      let is_struct = c.category != "enum";
      let variants = c
        .variants
        .into_iter()
        .map(|v| {
          is_struct
            .then(|| serde_json::from_value(v.definition))
            .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;
      conflicts.insert(c.name, variants);
    }
    Ok(Self { structs, conflicts })
  }

  /// Every unambiguous definition name, plus each conflicted name once.
  pub fn names(&self) -> impl Iterator<Item = &str> {
    self
      .structs
      .keys()
      .chain(self.conflicts.keys())
      .map(String::as_str)
  }

  /// `None` when `name` is conflicted: a bare conflicted name doesn't identify a layout.
  pub fn struct_by_name(&self, name: &str) -> Option<&StructDef> {
    self.structs.get(name)
  }

  pub fn is_conflicted(&self, name: &str) -> bool {
    self.conflicts.contains_key(name)
  }

  fn lookup(&self, name: &str, variant: Option<usize>) -> Option<&StructDef> {
    match variant {
      Some(v) => self.conflicts.get(name)?.get(v)?.as_ref(),
      None => self.structs.get(name),
    }
  }

  /// The definition a non-pointer aggregate type refers to.
  pub fn resolve(&self, ty: &TypeRef) -> Option<Cow<'_, StructDef>> {
    if let Some(inline) = &ty.inline {
      return serde_json::from_value(inline.clone()).ok().map(Cow::Owned);
    }
    self.lookup(&ty.base, ty.variant).map(Cow::Borrowed)
  }

  pub fn resolve_base(&self, base: &BaseRef) -> Option<&StructDef> {
    self.lookup(&base.name, base.variant)
  }

  /// The definition for a `.bs` struct's decomp name: an exact match, or else the only
  /// definition with that unqualified name.
  pub fn find_struct(&self, name: &str) -> Result<&StructDef, String> {
    if let Some(def) = self.struct_by_name(name) {
      return Ok(def);
    }
    let want = short_name(name);
    // A qualified `.bs` name must match a qualified DWARF name: mwcc emits nested
    // types unscoped, so bare `Area` could be any class's `Area` (it's
    // `CScriptLayerManager::Area`, not `CWorldLayers::Area`).
    let qualified = name.split('<').next().unwrap_or(name).contains("::");
    let candidates: Vec<&str> = self
      .names()
      .filter(|n| short_name(n) == want && (!qualified || n.contains("::")))
      .collect();
    match candidates.as_slice() {
      [only] if !self.is_conflicted(only) => Ok(self.struct_by_name(only).unwrap()),
      [] => Err(format!("no struct `{name}` in the decomp")),
      many => Err(format!("`{name}` is ambiguous: {}", many.join(", "))),
    }
  }

  /// The member `name` of `def`, looking through anonymous unions/structs and then base
  /// classes (C++ name lookup order), with its offset from the start of `def`.
  fn find_member(&self, def: &StructDef, name: &str) -> Option<(u32, Member)> {
    if let Some(m) = def.members.iter().find(|m| m.name.as_deref() == Some(name)) {
      return Some((m.offset, m.clone()));
    }
    for m in def.members.iter().filter(|m| m.name.is_none()) {
      if m.ty.is_aggregate()
        && let Some(inner) = self.resolve(&m.ty)
        && let Some((offset, found)) = self.find_member(&inner, name)
      {
        return Some((m.offset + offset, found));
      }
    }
    def.bases.iter().find_map(|b| {
      let (offset, found) = self.find_member(self.resolve_base(b)?, name)?;
      Some((b.offset + offset, found))
    })
  }

  pub fn resolve_path(&self, def: &StructDef, path: &str) -> Result<ResolvedField, String> {
    let mut offset = 0;
    let mut current: Option<Member> = None;
    for part in parse_path(path)? {
      match part {
        PathPart::Field(name) => {
          let found = match &current {
            None => self.find_member(def, name),
            Some(m) => {
              if !m.ty.is_aggregate() {
                return Err(format!("`{}` has no fields", m.ty.display));
              }
              let inner = self
                .resolve(&m.ty)
                .ok_or_else(|| format!("no layout for `{}`", m.ty.display))?;
              self.find_member(&inner, name)
            }
          };
          let (at, m) = found.ok_or_else(|| format!("no field `{name}`"))?;
          offset += at;
          current = Some(m);
        }
        PathPart::Index(i) => {
          let m = current
            .as_mut()
            .filter(|m| m.ty.is_array())
            .ok_or_else(|| format!("`[{i}]` in `{path}` doesn't index an array"))?;
          let count = m.ty.array_dims[0];
          if i >= count {
            return Err(format!("`[{i}]` is out of bounds (length {count})"));
          }
          let stride = m.byte_size.unwrap_or(0) / count;
          offset += i * stride;
          m.byte_size = Some(stride);
          m.ty.array_dims.remove(0);
        }
      }
    }
    let current = current.expect("a path has at least one field");
    Ok(ResolvedField {
      offset: offset as i64,
      size: current.byte_size.unwrap_or(0) as i64,
      bits: current
        .bit
        .map(|b| (b.bit_offset as i64, b.bit_size as i64)),
    })
  }
}

impl FieldResolver for LayoutDb {
  fn resolve_field(&self, decomp_struct: &str, path: &str) -> Result<ResolvedField, String> {
    self.resolve_path(self.find_struct(decomp_struct)?, path)
  }

  fn struct_size(&self, decomp_struct: &str) -> Option<i64> {
    self
      .find_struct(decomp_struct)
      .ok()?
      .byte_size
      .map(i64::from)
  }
}

/// Add a revision here once `tools/gen_decomp_data.sh` has generated its layouts.
fn source(version: GameVersion) -> Option<&'static [u8]> {
  match version {
    GameVersion::NtscU0_00 => Some(include_bytes!("../../prime_defs/layouts/GM8E01_00.json.gz")),
    _ => None,
  }
}

impl GameVersion {
  /// The revision whose layouts `.bs` field references resolve against: this one if its
  /// layouts have been generated, otherwise the default.
  pub fn layout_version(self) -> GameVersion {
    if source(self).is_some() {
      self
    } else {
      GameVersion::default()
    }
  }

  /// Layouts of [`GameVersion::layout_version`], parsed on first use.
  pub fn layouts(self) -> &'static LayoutDb {
    static PARSED: [OnceLock<LayoutDb>; GameVersion::ALL.len()] =
      [const { OnceLock::new() }; GameVersion::ALL.len()];
    let version = self.layout_version();
    let index = GameVersion::ALL.iter().position(|v| *v == version).unwrap();
    PARSED[index].get_or_init(|| {
      LayoutDb::from_gz(source(version).unwrap())
        .unwrap_or_else(|e| panic!("layouts for {version}: {e}"))
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn resolve(s: &str, path: &str) -> Result<ResolvedField, String> {
    GameVersion::NtscU0_00.layouts().resolve_field(s, path)
  }

  fn offset(s: &str, path: &str) -> i64 {
    resolve(s, path).unwrap().offset
  }

  #[test]
  fn resolves_gm8e01_00_fields() {
    assert_eq!(offset("CStateManager", "mPlayer"), 0x84c);
    assert_eq!(offset("CEntity", "__vptr$"), 0);
    // Base-class member, through CPhysicsActor and CActor.
    assert_eq!(offset("CPlayer", "mTransform"), 0x34);
    assert_eq!(offset("CDamageVulnerability", "mNormal[3]"), 0xc);
    assert_eq!(offset("CModelData", "mAnimData.mItem"), 0x10);
    assert_eq!(offset("CStateManager", "mObjectLists.mData[4]"), 0x810);
    assert_eq!(
      resolve("CEntity", "mActive").unwrap(),
      ResolvedField {
        offset: 0x30,
        size: 1,
        bits: Some((0, 1))
      }
    );
    assert_eq!(resolve("CScriptTrigger", "mFlags").unwrap().size, 4);
    assert_eq!(
      GameVersion::NtscU0_00
        .layouts()
        .struct_size("CStateManager"),
      Some(0xf98)
    );
  }

  #[test]
  fn reports_bad_paths() {
    assert!(
      resolve("CNoSuchThing", "mX")
        .unwrap_err()
        .contains("no struct")
    );
    assert!(
      resolve("CStateManager", "mNope")
        .unwrap_err()
        .contains("mNope")
    );
    assert!(
      resolve("CDamageVulnerability", "mNormal[99]")
        .unwrap_err()
        .contains("out of bounds")
    );
    assert!(resolve("CStateManager", "mPlayer[0]").is_err());
    assert!(
      resolve("CStateManager", "mPlayer.mX").is_err(),
      "through a pointer"
    );
    assert!(resolve("CStateManager", "mPlayer..x").is_err());
  }

  #[test]
  fn other_revisions_fall_back_to_the_default_layouts() {
    assert_eq!(
      GameVersion::NtscU0_00.layout_version(),
      GameVersion::NtscU0_00
    );
    assert_eq!(GameVersion::Pal.layout_version(), GameVersion::NtscU0_00);
  }
}
