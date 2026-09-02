//! Ports `../primewatch2/src/defs/EItemType.{hpp,cpp}` — the pickup item-type
//! enum (`EItemType.hpp:5-51`) and its display-name table
//! (`EItemType.cpp:3-90`).

/// Ports `enum class EItemType` (`EItemType.hpp:5-51`). Discriminants match the
/// C++ 1:1 (`Invalid = -1`, `PowerBeam = 0` … `Newborn = 40`, `Max = 41`).
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EItemType {
  Invalid = -1,
  PowerBeam = 0,
  IceBeam = 1,
  WaveBeam = 2,
  PlasmaBeam = 3,
  Missiles = 4,
  ScanVisor = 5,
  MorphBallBombs = 6,
  PowerBombs = 7,
  Flamethrower = 8,
  ThermalVisor = 9,
  ChargeBeam = 10,
  SuperMissile = 11,
  GrappleBeam = 12,
  XRayVisor = 13,
  IceSpreader = 14,
  SpaceJumpBoots = 15,
  MorphBall = 16,
  CombatVisor = 17,
  BoostBall = 18,
  SpiderBall = 19,
  PowerSuit = 20,
  GravitySuit = 21,
  VariaSuit = 22,
  PhazonSuit = 23,
  EnergyTanks = 24,
  UnknownItem1 = 25,
  HealthRefill = 26,
  UnknownItem2 = 27,
  Wavebuster = 28,
  Truth = 29,
  Strength = 30,
  Elder = 31,
  Wild = 32,
  Lifegiver = 33,
  Warrior = 34,
  Chozo = 35,
  Nature = 36,
  Sun = 37,
  World = 38,
  Spirit = 39,
  Newborn = 40,
  /// This must remain at the end of the list (C++ `Max`).
  Max = 41,
}

impl EItemType {
  /// Ports the C++ `static_cast<EItemType>(pickup["itemType"].read_u32())`
  /// (`WorldRenderer.cpp:947`): a raw memory value into the enum. Anything not a
  /// named variant maps to [`EItemType::Invalid`], which
  /// [`item_type_to_name`] renders as `"Unknown"` — same as the C++ switch
  /// `default`.
  pub fn from_raw(v: u32) -> Self {
    match v {
      0 => Self::PowerBeam,
      1 => Self::IceBeam,
      2 => Self::WaveBeam,
      3 => Self::PlasmaBeam,
      4 => Self::Missiles,
      5 => Self::ScanVisor,
      6 => Self::MorphBallBombs,
      7 => Self::PowerBombs,
      8 => Self::Flamethrower,
      9 => Self::ThermalVisor,
      10 => Self::ChargeBeam,
      11 => Self::SuperMissile,
      12 => Self::GrappleBeam,
      13 => Self::XRayVisor,
      14 => Self::IceSpreader,
      15 => Self::SpaceJumpBoots,
      16 => Self::MorphBall,
      17 => Self::CombatVisor,
      18 => Self::BoostBall,
      19 => Self::SpiderBall,
      20 => Self::PowerSuit,
      21 => Self::GravitySuit,
      22 => Self::VariaSuit,
      23 => Self::PhazonSuit,
      24 => Self::EnergyTanks,
      25 => Self::UnknownItem1,
      26 => Self::HealthRefill,
      27 => Self::UnknownItem2,
      28 => Self::Wavebuster,
      29 => Self::Truth,
      30 => Self::Strength,
      31 => Self::Elder,
      32 => Self::Wild,
      33 => Self::Lifegiver,
      34 => Self::Warrior,
      35 => Self::Chozo,
      36 => Self::Nature,
      37 => Self::Sun,
      38 => Self::World,
      39 => Self::Spirit,
      40 => Self::Newborn,
      41 => Self::Max,
      _ => Self::Invalid,
    }
  }
}

/// Ports `itemTypeToName` (`EItemType.cpp:3-90`). The C++ switch has no cases for
/// `Invalid` / `Max`, so they fall to `default` -> `"Unknown"`.
pub fn item_type_to_name(t: EItemType) -> &'static str {
  match t {
    EItemType::PowerBeam => "Power Beam",
    EItemType::IceBeam => "Ice Beam",
    EItemType::WaveBeam => "Wave Beam",
    EItemType::PlasmaBeam => "Plasma Beam",
    EItemType::Missiles => "Missiles",
    EItemType::ScanVisor => "Scan Visor",
    EItemType::MorphBallBombs => "Morph Ball Bombs",
    EItemType::PowerBombs => "Power Bombs",
    EItemType::Flamethrower => "Flamethrower",
    EItemType::ThermalVisor => "Thermal Visor",
    EItemType::ChargeBeam => "Charge Beam",
    EItemType::SuperMissile => "Super Missile",
    EItemType::GrappleBeam => "Grapple Beam",
    EItemType::XRayVisor => "X-Ray Visor",
    EItemType::IceSpreader => "Ice Spreader",
    EItemType::SpaceJumpBoots => "Space Jump Boots",
    EItemType::MorphBall => "Morph Ball",
    EItemType::CombatVisor => "Combat Visor",
    EItemType::BoostBall => "Boost Ball",
    EItemType::SpiderBall => "Spider Ball",
    EItemType::PowerSuit => "Power Suit",
    EItemType::GravitySuit => "Gravity Suit",
    EItemType::VariaSuit => "Varia Suit",
    EItemType::PhazonSuit => "Phazon Suit",
    EItemType::EnergyTanks => "Energy Tanks",
    EItemType::UnknownItem1 => "Unknown Item 1",
    EItemType::HealthRefill => "Health Refill",
    EItemType::UnknownItem2 => "Unknown Item 2",
    EItemType::Wavebuster => "Wavebuster",
    EItemType::Truth => "Truth",
    EItemType::Strength => "Strength",
    EItemType::Elder => "Elder",
    EItemType::Wild => "Wild",
    EItemType::Lifegiver => "Lifegiver",
    EItemType::Warrior => "Warrior",
    EItemType::Chozo => "Chozo",
    EItemType::Nature => "Nature",
    EItemType::Sun => "Sun",
    EItemType::World => "World",
    EItemType::Spirit => "Spirit",
    EItemType::Newborn => "Newborn",
    EItemType::Invalid | EItemType::Max => "Unknown",
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn from_raw_maps_named_values_and_falls_back() {
    assert_eq!(EItemType::from_raw(0), EItemType::PowerBeam);
    assert_eq!(EItemType::from_raw(7), EItemType::PowerBombs);
    assert_eq!(EItemType::from_raw(40), EItemType::Newborn);
    assert_eq!(EItemType::from_raw(41), EItemType::Max);
    assert_eq!(EItemType::from_raw(9999), EItemType::Invalid);
    assert_eq!(EItemType::from_raw(0xFFFF_FFFF), EItemType::Invalid);
  }

  #[test]
  fn names_match_cpp_switch() {
    assert_eq!(item_type_to_name(EItemType::PowerBeam), "Power Beam");
    assert_eq!(
      item_type_to_name(EItemType::MorphBallBombs),
      "Morph Ball Bombs"
    );
    assert_eq!(item_type_to_name(EItemType::XRayVisor), "X-Ray Visor");
    assert_eq!(item_type_to_name(EItemType::UnknownItem1), "Unknown Item 1");
    assert_eq!(item_type_to_name(EItemType::Newborn), "Newborn");
    assert_eq!(item_type_to_name(EItemType::Invalid), "Unknown");
    assert_eq!(item_type_to_name(EItemType::Max), "Unknown");
  }

  #[test]
  fn discriminants_match_cpp() {
    assert_eq!(EItemType::Invalid as i32, -1);
    assert_eq!(EItemType::PowerBeam as i32, 0);
    assert_eq!(EItemType::Newborn as i32, 40);
    assert_eq!(EItemType::Max as i32, 41);
  }
}
