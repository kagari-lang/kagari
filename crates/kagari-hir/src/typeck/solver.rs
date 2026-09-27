use std::collections::HashMap;

use kagari_common::cancellation::{CancellationToken, Cancelled};

use crate::{hir::ExprId, types::TypeId};

/// Constraint storage belongs to one function body, never to a cached signature.
#[derive(Default)]
pub(super) struct Solver {
    variables: HashMap<(ExprId, usize), u32>,
    bindings: Vec<Option<TypeId>>,
    numeric: HashMap<u32, crate::types::BuiltinType>,
    pub revision: usize,
}

impl Solver {
    pub fn numeric_variable(
        &mut self,
        site: ExprId,
        fallback: crate::types::BuiltinType,
    ) -> TypeId {
        let variable = self.variable(site, 1);
        let TypeId::Inference(id) = variable else {
            unreachable!()
        };
        self.numeric.insert(id, fallback);
        self.resolve(&variable)
    }

    pub fn apply_numeric_defaults(&mut self) -> bool {
        let mut defaults = self
            .numeric
            .iter()
            .map(|(id, ty)| (*id, *ty))
            .collect::<Vec<_>>();
        defaults.sort_by_key(|(id, _)| *id);
        let before = self.revision;
        for (id, fallback) in defaults {
            if let TypeId::Inference(root) = self.resolve(&TypeId::Inference(id)) {
                self.bindings[root as usize] = Some(TypeId::Builtin(fallback));
                self.revision += 1;
            }
        }
        self.revision != before
    }
    pub fn variable(&mut self, site: ExprId, slot: usize) -> TypeId {
        let next = self.bindings.len() as u32;
        let id = *self.variables.entry((site, slot)).or_insert_with(|| {
            self.bindings.push(None);
            next
        });
        TypeId::Inference(id)
    }

    pub fn resolve(&self, ty: &TypeId) -> TypeId {
        let mut result = ty.clone();
        // Occurs checks guarantee an acyclic substitution graph.
        for _ in 0..=self.bindings.len() {
            let next = result.substitute_once(|ty| match ty {
                TypeId::Inference(id) => self.bindings[*id as usize].as_ref(),
                _ => None,
            });
            if next == result {
                return result;
            }
            result = next;
        }
        unreachable!("acyclic inference substitutions");
    }

    /// Accumulate compatible structural facts. Conflicts remain for the checked
    /// pass to diagnose at their original expression and parameter locations.
    pub fn constrain(
        &mut self,
        left: &TypeId,
        right: &TypeId,
        cancel: &CancellationToken,
    ) -> Result<bool, Cancelled> {
        let mut pending = vec![(left.clone(), right.clone())];
        let mut compatible = true;
        while let Some((left, right)) = pending.pop() {
            cancel.check()?;
            let left = self.resolve(&left);
            let right = self.resolve(&right);
            if left == right {
                continue;
            }
            match (&left, &right) {
                (TypeId::Unknown | TypeId::Error, _) | (_, TypeId::Unknown | TypeId::Error) => {}
                (TypeId::Inference(id), ty) | (ty, TypeId::Inference(id)) => {
                    if let Some(fallback) = self.numeric.get(id).copied() {
                        let floating = fallback == crate::types::BuiltinType::F64;
                        let admissible = match ty {
                            TypeId::Inference(other) => {
                                self.numeric.get(other).is_none_or(|other| {
                                    (*other == crate::types::BuiltinType::F64) == floating
                                })
                            }
                            TypeId::Builtin(target) => {
                                use crate::types::BuiltinType::*;
                                if floating {
                                    matches!(target, F32 | F64)
                                } else {
                                    matches!(
                                        target,
                                        I8 | I16 | I32 | I64 | ISize | U8 | U16 | U32 | U64 | USize
                                    )
                                }
                            }
                            _ => false,
                        };
                        if !admissible {
                            compatible = false;
                            continue;
                        }
                        if let TypeId::Inference(other) = ty {
                            self.numeric.insert(*other, fallback);
                        }
                    }
                    let mut occurs = false;
                    ty.substitute_once(|member| {
                        occurs |= matches!(member, TypeId::Inference(other) if other == id);
                        None
                    });
                    if occurs {
                        compatible = false;
                    } else {
                        self.bindings[*id as usize] = Some(ty.clone());
                        self.revision += 1;
                    }
                }
                _ => {
                    let same_shape = match (&left, &right) {
                        (TypeId::Struct(a), TypeId::Struct(b))
                        | (TypeId::Enum(a), TypeId::Enum(b))
                        | (TypeId::Trait(a), TypeId::Trait(b)) => {
                            a.declaration == b.declaration
                                && a.arguments.len() == b.arguments.len()
                                && a.associated_types.keys().eq(b.associated_types.keys())
                        }
                        (TypeId::Tuple(a), TypeId::Tuple(b)) => a.len() == b.len(),
                        (
                            TypeId::StandardEnum { kind: a, args: aa },
                            TypeId::StandardEnum { kind: b, args: ba },
                        ) => a == b && aa.len() == ba.len(),
                        (
                            TypeId::Function { params: a, .. },
                            TypeId::Function { params: b, .. },
                        ) => a.len() == b.len(),
                        (TypeId::Array(..), TypeId::Array(..))
                        | (TypeId::Set(..), TypeId::Set(..))
                        | (TypeId::Map { .. }, TypeId::Map { .. })
                        | (TypeId::Iter(_), TypeId::Iter(_)) => true,
                        // Projections are not injective. Only the ordinary
                        // associated-type normalizer may reduce them.
                        _ => false,
                    };
                    if same_shape {
                        let mut a = Vec::new();
                        let mut b = Vec::new();
                        left.map_children(|ty| {
                            a.push(ty.clone());
                            ty.clone()
                        });
                        right.map_children(|ty| {
                            b.push(ty.clone());
                            ty.clone()
                        });
                        pending.extend(a.into_iter().zip(b));
                    } else {
                        compatible = false;
                    }
                }
            }
        }
        Ok(compatible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::BuiltinType;
    use kagari_common::collection::CollectionAccess;

    #[test]
    fn later_constraints_resolve_nested_types_without_conflating_recovery() {
        let mut solver = Solver::default();
        // Variables are allocated directly here to avoid depending on HIR arenas.
        solver.bindings = vec![None, None];
        let a = TypeId::Inference(0);
        let b = TypeId::Inference(1);
        let array = TypeId::Array(Box::new(b.clone()), CollectionAccess::Mutable);
        let cancel = CancellationToken::default();
        assert!(solver.constrain(&a, &array, &cancel).unwrap());
        assert!(solver.constrain(&b, &TypeId::Unknown, &cancel).unwrap());
        assert_eq!(solver.resolve(&b), b);
        let integer = TypeId::Builtin(BuiltinType::I32);
        assert!(solver.constrain(&b, &integer, &cancel).unwrap());
        assert_eq!(
            solver.resolve(&a),
            TypeId::Array(Box::new(integer), CollectionAccess::Mutable)
        );
    }

    #[test]
    fn recursive_constraints_are_rejected_without_installing_a_cycle() {
        let mut solver = Solver::default();
        solver.bindings = vec![None];
        let variable = TypeId::Inference(0);
        let nested = TypeId::Tuple(vec![variable.clone()]);
        assert!(
            !solver
                .constrain(&variable, &nested, &CancellationToken::default())
                .unwrap()
        );
        assert_eq!(solver.resolve(&variable), variable);
    }

    #[test]
    fn cancelled_constraints_do_not_change_bindings() {
        let mut solver = Solver {
            bindings: vec![None],
            ..Default::default()
        };
        let cancel = CancellationToken::default();
        cancel.cancel();
        assert_eq!(
            solver.constrain(
                &TypeId::Inference(0),
                &TypeId::Builtin(BuiltinType::I32),
                &cancel
            ),
            Err(Cancelled)
        );
        assert_eq!(solver.revision, 0);
    }
}
