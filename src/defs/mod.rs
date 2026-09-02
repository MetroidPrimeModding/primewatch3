//! Hand-ported game enums / constants that have no `.bs` schema representation.
//!
//! Ports `../primewatch2/src/defs/*` — the C++ side keeps these as plain C++
//! `enum class` + switch tables, separate from the schema registry.

pub mod item_types;
