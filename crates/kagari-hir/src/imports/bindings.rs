//! Independent type/value slots for bindings and import outcomes.
use crate::imports::ResolvedTarget;
use kagari_types::declaration::names::NameNamespace;
use std::ops::{Index, IndexMut};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PerNamespace<T> {
    pub types: T,
    pub values: T,
}

impl<T> PerNamespace<T> {
    pub fn iter(&self) -> impl Iterator<Item = (NameNamespace, &T)> {
        [
            (NameNamespace::Type, &self.types),
            (NameNamespace::Value, &self.values),
        ]
        .into_iter()
    }

    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        [&mut self.types, &mut self.values].into_iter()
    }
}

impl<T> Index<NameNamespace> for PerNamespace<T> {
    type Output = T;

    fn index(&self, namespace: NameNamespace) -> &T {
        match namespace {
            NameNamespace::Type => &self.types,
            NameNamespace::Value => &self.values,
        }
    }
}

impl<T> IndexMut<NameNamespace> for PerNamespace<T> {
    fn index_mut(&mut self, namespace: NameNamespace) -> &mut T {
        match namespace {
            NameNamespace::Type => &mut self.types,
            NameNamespace::Value => &mut self.values,
        }
    }
}

/// Per-category result; absence releases only this category's reserved slot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LookupOutcome {
    #[default]
    Pending,
    Absent,
    Resolved(ResolvedTarget),
    Inaccessible,
    Ambiguous,
    NotNamespace,
    StaleSource,
}

impl LookupOutcome {
    pub fn target(&self) -> Option<&ResolvedTarget> {
        match self {
            Self::Resolved(target) => Some(target),
            _ => None,
        }
    }
}
