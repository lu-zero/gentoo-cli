//! Package identity types for the PubGrub solver.
//!
//! ## Solver-internal bookkeeping nodes
//!
//! PubGrub requires every dependency constraint to be expressed as a
//! *package + version range* pair, and it must start resolution from a single
//! root package.  Portage has no equivalent concepts, so we use enum variants
//! to distinguish real packages from synthetic solver nodes:
//!
//! | Variant | Purpose |
//! |---|---|
//! | `Real` | An actual Gentoo package (CPN + optional slot) |
//! | `Root` | Synthetic root; its "deps" are the caller's resolve targets |
//! | `UseDecision` | USE-flag decision node; version 1 = enabled, 0 = disabled |
//! | `Choice` | OR-group (`||`, `^^`, `??`) node; each version is one alternative |
//! | `SlotChoice` | Slot-choice node; each version selects one slot candidate |
//!
//! Virtual variants exist only inside the solver and are **always stripped**
//! from [`PortageDependencyProvider::resolve_targets`] before the result is
//! returned.  No caller ever sees them.

use std::fmt;

use portage_atom::Cpn;
use portage_atom::interner::{DefaultInterner, Interned};

/// A PubGrub-compatible package identifier.
///
/// Real packages carry a CPN and optional slot.  Virtual variants
/// (`Root`, `UseDecision`, `Choice`, `SlotChoice`) are solver-internal
/// bookkeeping nodes.
///
/// Implements `Clone + Eq + Hash + Debug + Display`, satisfying pubgrub's
/// `Package` trait via blanket implementation.
///
/// See [PMS 8.3.3](https://projects.gentoo.org/pms/9/pms.html#slot_deps).
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub enum PortagePackage {
    /// A real Gentoo package, identified by CPN + optional slot.
    Real {
        cpn: Cpn,
        slot: Option<Interned<DefaultInterner>>,
    },
    /// Synthetic root node for the solver.
    Root,
    /// USE-flag decision node.  Version 1 = enabled, 0 = disabled.
    UseDecision { name: Interned<DefaultInterner> },
    /// OR-group choice node.  Each version is one alternative.
    Choice { name: Interned<DefaultInterner> },
    /// Slot-choice node.  Each version selects one slot candidate.
    SlotChoice { name: Interned<DefaultInterner> },
}

impl PortagePackage {
    /// Create a real package from a CPN and optional slot.
    pub fn new(cpn: Cpn, slot: Option<Interned<DefaultInterner>>) -> Self {
        Self::Real { cpn, slot }
    }

    /// Create an unslotted real package.
    pub fn unslotted(cpn: Cpn) -> Self {
        Self::Real { cpn, slot: None }
    }

    /// Create a slotted real package.
    pub fn slotted(cpn: Cpn, slot: Interned<DefaultInterner>) -> Self {
        Self::Real {
            cpn,
            slot: Some(slot),
        }
    }

    /// Create the solver-internal root node.
    pub(crate) fn synthetic_root() -> Self {
        Self::Root
    }

    /// Create a USE-flag decision node.
    pub(crate) fn use_decision(name: Interned<DefaultInterner>) -> Self {
        Self::UseDecision { name }
    }

    /// Create an OR-group choice node.
    pub(crate) fn choice(name: Interned<DefaultInterner>) -> Self {
        Self::Choice { name }
    }

    /// Create a slot-choice node.
    pub(crate) fn slot_choice(name: Interned<DefaultInterner>) -> Self {
        Self::SlotChoice { name }
    }

    /// Returns `true` for solver-internal virtual variants.
    pub fn is_virtual(&self) -> bool {
        !matches!(self, Self::Real { .. })
    }

    /// Returns the CPN for real packages.
    ///
    /// # Panics
    /// Panics if called on a virtual variant.
    pub fn cpn(&self) -> &Cpn {
        match self {
            Self::Real { cpn, .. } => cpn,
            _ => panic!("cpn() called on virtual package {:?}", self),
        }
    }

    /// Returns the slot for real packages.
    pub fn slot(&self) -> Option<Interned<DefaultInterner>> {
        match self {
            Self::Real { slot, .. } => *slot,
            _ => None,
        }
    }

    /// Returns the display string without slot suffix.
    pub fn cpn_str(&self) -> String {
        self.cpn().to_string()
    }
}

impl Ord for PortagePackage {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self, other) {
            (
                Self::Real {
                    cpn: a_cpn,
                    slot: a_slot,
                },
                Self::Real {
                    cpn: b_cpn,
                    slot: b_slot,
                },
            ) => a_cpn.cmp(b_cpn).then_with(|| match (a_slot, b_slot) {
                (Some(a), Some(b)) => a.as_str().cmp(b.as_str()),
                (Some(_), None) => Ordering::Greater,
                (None, Some(_)) => Ordering::Less,
                (None, None) => Ordering::Equal,
            }),
            (Self::Real { .. }, _) => Ordering::Less,
            (_, Self::Real { .. }) => Ordering::Greater,
            _ => self.discriminant_ord().cmp(&other.discriminant_ord()),
        }
    }
}

impl PartialOrd for PortagePackage {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PortagePackage {
    fn discriminant_ord(&self) -> u8 {
        match self {
            Self::Real { .. } => 0,
            Self::Root => 1,
            Self::UseDecision { .. } => 2,
            Self::Choice { .. } => 3,
            Self::SlotChoice { .. } => 4,
        }
    }
}

impl fmt::Display for PortagePackage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Real {
                cpn,
                slot: Some(slot),
            } => write!(f, "{}:{}", cpn, slot),
            Self::Real { cpn, slot: None } => write!(f, "{}", cpn),
            Self::Root => write!(f, "__internal__/root"),
            Self::UseDecision { name } => write!(f, "__internal__/{}", name),
            Self::Choice { name } => write!(f, "__internal__/{}", name),
            Self::SlotChoice { name } => write!(f, "__internal__/{}", name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_unslotted() {
        let cpn = Cpn::parse("dev-lang/rust").unwrap();
        let pkg = PortagePackage::unslotted(cpn);
        assert_eq!(pkg.to_string(), "dev-lang/rust");
    }

    #[test]
    fn display_slotted() {
        let cpn = Cpn::parse("dev-lang/python").unwrap();
        let slot = Interned::intern("3.12");
        let pkg = PortagePackage::slotted(cpn, slot);
        assert_eq!(pkg.to_string(), "dev-lang/python:3.12");
    }

    #[test]
    fn different_slots_are_different_packages() {
        let cpn = Cpn::parse("dev-lang/python").unwrap();
        let p1 = PortagePackage::slotted(cpn, Interned::intern("3.11"));
        let p2 = PortagePackage::slotted(
            Cpn::parse("dev-lang/python").unwrap(),
            Interned::intern("3.12"),
        );
        assert_ne!(p1, p2);
    }

    #[test]
    fn same_slot_is_same_package() {
        let cpn = Cpn::parse("dev-lang/python").unwrap();
        let p1 = PortagePackage::slotted(cpn, Interned::intern("3.12"));
        let p2 = PortagePackage::slotted(
            Cpn::parse("dev-lang/python").unwrap(),
            Interned::intern("3.12"),
        );
        assert_eq!(p1, p2);
    }

    #[test]
    fn real_sorts_before_virtual() {
        let cpn = Cpn::parse("dev-lang/rust").unwrap();
        let real = PortagePackage::unslotted(cpn);
        let root = PortagePackage::synthetic_root();
        assert!(real < root);
    }

    #[test]
    fn is_virtual() {
        let cpn = Cpn::parse("dev-lang/rust").unwrap();
        assert!(!PortagePackage::unslotted(cpn).is_virtual());
        assert!(PortagePackage::synthetic_root().is_virtual());
        assert!(PortagePackage::use_decision(Interned::intern("test")).is_virtual());
    }
}
