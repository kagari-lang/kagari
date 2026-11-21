//! Bounded, two-sided structural unification for coherence, not proof selection.
use crate::ty::{
    Ty,
    substitution::{MAX_TYPE_DEPTH, MAX_TYPE_NODES},
};
use kagari_common::identity::table::DefinitionId;
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Side {
    Left,
    Right,
}

type Binder = (Side, DefinitionId, usize);

#[derive(Clone, Copy)]
struct Term<'a> {
    side: Side,
    ty: &'a Ty,
}

impl Term<'_> {
    fn binder(self) -> Option<Binder> {
        match self.ty {
            Ty::Parameter { owner, position } => Some((self.side, *owner, *position)),
            _ => None,
        }
    }
}

pub(super) struct Comparison<'a> {
    bindings: BTreeMap<Binder, Term<'a>>,
    remaining: usize,
}

impl<'a> Comparison<'a> {
    pub(super) fn new() -> Self {
        Self {
            bindings: BTreeMap::new(),
            remaining: MAX_TYPE_NODES * 2,
        }
    }

    pub(super) fn pair(&mut self, left: &'a Ty, right: &'a Ty) -> Option<bool> {
        self.compare(
            Term {
                side: Side::Left,
                ty: left,
            },
            Term {
                side: Side::Right,
                ty: right,
            },
            1,
        )
    }

    fn resolve(&mut self, mut term: Term<'a>) -> Option<Term<'a>> {
        while let Some(binding) = term.binder().and_then(|key| self.bindings.get(&key)) {
            self.remaining = self.remaining.checked_sub(1)?;
            term = *binding;
        }
        Some(term)
    }

    fn compare(&mut self, left: Term<'a>, right: Term<'a>, depth: usize) -> Option<bool> {
        if depth > MAX_TYPE_DEPTH {
            return None;
        }
        self.remaining = self.remaining.checked_sub(1)?;
        let left = self.resolve(left)?;
        let right = self.resolve(right)?;
        if matches!(left.ty, Ty::Projection { .. }) || matches!(right.ty, Ty::Projection { .. }) {
            return Some(true);
        }
        // Link only unresolved roots. A recursive structural equation need not
        // be disproved: revisiting its expansion is subject to both budgets.
        if let Some(key) = left.binder() {
            if Some(key) != right.binder() {
                self.bindings.insert(key, right);
            }
            return Some(true);
        }
        if let Some(key) = right.binder() {
            self.bindings.insert(key, left);
            return Some(true);
        }

        let mut child =
            |a, b| self.compare(Term { ty: a, ..left }, Term { ty: b, ..right }, depth + 1);
        let (a, b) = match (left.ty, right.ty) {
            (Ty::Builtin(a), Ty::Builtin(b)) => return Some(a == b),
            (Ty::Host(a), Ty::Host(b)) | (Ty::SelfType(a), Ty::SelfType(b)) => return Some(a == b),
            (Ty::Array(a), Ty::Array(b)) => return child(a, b),
            (Ty::Range(a, ak), Ty::Range(b, bk)) if ak == bk => return child(a, b),
            (Ty::Tuple(a), Ty::Tuple(b)) => (a, b),
            (
                Ty::Function {
                    params: a,
                    result: ar,
                },
                Ty::Function {
                    params: b,
                    result: br,
                },
            ) => {
                if a.len() != b.len() || !child(ar, br)? {
                    return Some(false);
                }
                (a, b)
            }
            (Ty::Struct(a), Ty::Struct(b))
            | (Ty::Enum(a), Ty::Enum(b))
            | (Ty::Trait(a), Ty::Trait(b))
            | (Ty::NativeObject(a), Ty::NativeObject(b)) => {
                if a.declaration != b.declaration
                    || a.associated_types.len() != b.associated_types.len()
                {
                    return Some(false);
                }
                for ((ak, av), (bk, bv)) in a.associated_types.iter().zip(&b.associated_types) {
                    if ak != bk || !child(av, bv)? {
                        return Some(false);
                    }
                }
                (&a.arguments, &b.arguments)
            }
            _ => return Some(false),
        };
        if a.len() != b.len() {
            return Some(false);
        }
        for (a, b) in a.iter().zip(b) {
            if !child(a, b)? {
                return Some(false);
            }
        }
        Some(true)
    }
}
