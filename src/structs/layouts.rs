//! Per-revision type layouts exported from a `-g` decomp build by `dtk dwarf types`
//! (`prime_defs/layouts/<VERSION>.json.gz`, see `doc/multi-version.md`).
//!
//! mwcc's DWARF 1.1 names types by bare identifier, so many template instantiations
//! share a name (`vector`, `single_ptr`, ...). dtk lists those under `conflicts`,
//! and every type reference to one carries `variant`, the index of the exact
//! definition it uses. Always resolve through [`LayoutDb::resolve`] rather than
//! looking a bare name up directly.

use serde::Deserialize;
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::Read;

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

impl LayoutDb {
  pub fn load_gz(path: &str) -> Result<Self, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut json = String::new();
    flate2::read::GzDecoder::new(file)
      .read_to_string(&mut json)
      .map_err(|e| format!("{path}: {e}"))?;
    Self::from_json(&json).map_err(|e| format!("{path}: {e}"))
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
}
