use crate::types::{GenericParameterType, TypeId, TypeSubstitution};

/// Infer only the callee's parameters. Repeated occurrences are checked against
/// the resulting signature by the caller; no source spelling participates.
pub(super) fn infer(
    expected: &TypeId,
    actual: &TypeId,
    parameters: &[GenericParameterType],
    substitution: &mut TypeSubstitution,
    cancel: &kagari_common::cancellation::CancellationToken,
) -> Result<(), kagari_common::cancellation::Cancelled> {
    let mut pending = vec![(expected, actual)];
    while let Some((expected, actual)) = pending.pop() {
        cancel.check()?;
        if matches!(actual, TypeId::Unknown | TypeId::Error) {
            continue;
        }
        match (expected, actual) {
            (TypeId::Struct(expected), TypeId::Struct(actual))
            | (TypeId::Enum(expected), TypeId::Enum(actual))
            | (TypeId::Trait(expected), TypeId::Trait(actual))
                if expected.declaration == actual.declaration
                    && expected.arguments.len() == actual.arguments.len() =>
            {
                pending.extend(expected.arguments.iter().zip(&actual.arguments).rev());
            }
            (TypeId::Generic(parameter), _) if parameters.contains(parameter) => {
                substitution
                    .entry(parameter.clone())
                    .and_modify(|inferred| inferred.recover_from(actual))
                    .or_insert_with(|| actual.clone());
            }
            (TypeId::Tuple(expected), TypeId::Tuple(actual)) if expected.len() == actual.len() => {
                pending.extend(expected.iter().zip(actual).rev());
            }
            (
                TypeId::StandardEnum {
                    kind: expected_kind,
                    args: expected,
                },
                TypeId::StandardEnum {
                    kind: actual_kind,
                    args: actual,
                },
            ) if expected_kind == actual_kind && expected.len() == actual.len() => {
                pending.extend(expected.iter().zip(actual).rev());
            }
            (TypeId::Array(expected), TypeId::Array(actual))
            | (TypeId::Set(expected), TypeId::Set(actual)) => {
                pending.push((expected, actual));
            }
            (TypeId::Map { key: ek, value: ev }, TypeId::Map { key: ak, value: av }) => {
                pending.push((ev, av));
                pending.push((ek, ak));
            }
            (
                TypeId::Function {
                    params: ep,
                    result: er,
                },
                TypeId::Function {
                    params: ap,
                    result: ar,
                },
            ) if ep.len() == ap.len() => {
                pending.push((er, ar));
                pending.extend(ep.iter().zip(ap).rev());
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BuiltinType, NominalType};
    use kagari_common::identity::{
        DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity,
    };

    #[test]
    fn deep_inference_is_iterative_cancellable_and_preserves_member_order() {
        let parameter = GenericParameterType {
            owner: DefinitionId {
                module: ModuleIdentity::single_file("deep.kgr"),
                path: vec![DefinitionPathSegment {
                    kind: DefinitionKind::Function,
                    name: "infer".into(),
                    occurrence: 0,
                }],
            },
            position: 0,
            name: "T".into(),
        };
        let mut expected = TypeId::Generic(parameter.clone());
        let mut actual = TypeId::Builtin(BuiltinType::I32);
        for _ in 0..10_000 {
            expected = TypeId::Array(Box::new(expected));
            actual = TypeId::Array(Box::new(actual));
        }
        let mut substitution = TypeSubstitution::new();
        let cancelled = kagari_common::cancellation::CancellationToken::default();
        cancelled.cancel();
        infer(
            &expected,
            &actual,
            std::slice::from_ref(&parameter),
            &mut substitution,
            &cancelled,
        )
        .unwrap_err();
        assert!(substitution.is_empty());
        infer(
            &expected,
            &actual,
            std::slice::from_ref(&parameter),
            &mut substitution,
            &Default::default(),
        )
        .unwrap();
        assert_eq!(substitution[&parameter], TypeId::Builtin(BuiltinType::I32));
        // Drop the synthetic deep inputs iteratively too; this test isolates traversal.
        for mut ty in [expected, actual] {
            while let TypeId::Array(element) = ty {
                ty = *element;
            }
        }
        substitution.clear();
        infer(
            &TypeId::Tuple(vec![TypeId::Generic(parameter.clone()); 2]),
            &TypeId::Tuple(vec![
                TypeId::Builtin(BuiltinType::I32),
                TypeId::Builtin(BuiltinType::Bool),
            ]),
            std::slice::from_ref(&parameter),
            &mut substitution,
            &Default::default(),
        )
        .unwrap();
        assert_eq!(substitution[&parameter], TypeId::Builtin(BuiltinType::I32));
    }

    #[test]
    fn deep_recovery_and_conflicts_use_iterative_member_walks() {
        let mut recovering = TypeId::Error;
        let mut integer = TypeId::Builtin(BuiltinType::I32);
        let mut boolean = TypeId::Builtin(BuiltinType::Bool);
        for _ in 0..10_000 {
            recovering = TypeId::Array(Box::new(recovering));
            integer = TypeId::Array(Box::new(integer));
            boolean = TypeId::Array(Box::new(boolean));
        }
        assert!(!recovering.conflicts_with(&integer));
        recovering.recover_from(&integer);
        assert!(!recovering.conflicts_with(&integer));
        assert!(recovering.conflicts_with(&boolean));
        recovering.recover_from(&boolean);
        assert!(!recovering.conflicts_with(&integer));
        for mut ty in [recovering, integer, boolean] {
            while let TypeId::Array(element) = ty {
                ty = *element;
            }
        }
    }

    #[test]
    fn recovery_keeps_independent_facts_without_crossing_shape_boundaries() {
        let mut value = TypeId::Map {
            key: Box::new(TypeId::Builtin(BuiltinType::I32)),
            value: Box::new(TypeId::Tuple(vec![
                TypeId::Error,
                TypeId::Builtin(BuiltinType::Bool),
            ])),
        };
        let other = TypeId::Map {
            key: Box::new(TypeId::Builtin(BuiltinType::Bool)),
            value: Box::new(TypeId::Tuple(vec![
                TypeId::Builtin(BuiltinType::I32),
                TypeId::Builtin(BuiltinType::I32),
            ])),
        };
        value.recover_from(&other);
        assert_eq!(
            value,
            TypeId::Map {
                key: Box::new(TypeId::Builtin(BuiltinType::I32)),
                value: Box::new(TypeId::Tuple(vec![
                    TypeId::Builtin(BuiltinType::I32),
                    TypeId::Builtin(BuiltinType::Bool)
                ])),
            }
        );
        assert!(value.conflicts_with(&other));
        let mut different_arity = TypeId::Tuple(vec![TypeId::Error]);
        different_arity.recover_from(&TypeId::Tuple(vec![TypeId::Builtin(BuiltinType::I32); 2]));
        assert_eq!(different_arity, TypeId::Tuple(vec![TypeId::Error]));
        let declaration = DefinitionId {
            module: ModuleIdentity::single_file("a.kgr"),
            path: vec![],
        };
        let mut nominal = TypeId::Struct(NominalType {
            declaration: declaration.clone(),
            arguments: vec![TypeId::Error],
        });
        let before = nominal.clone();
        nominal.recover_from(&TypeId::Enum(NominalType {
            declaration: declaration.clone(),
            arguments: vec![TypeId::Builtin(BuiltinType::I32)],
        }));
        assert_eq!(nominal, before);
        let mut foreign = declaration;
        foreign.module = ModuleIdentity::single_file("b.kgr");
        nominal.recover_from(&TypeId::Struct(NominalType {
            declaration: foreign,
            arguments: vec![TypeId::Builtin(BuiltinType::I32)],
        }));
        assert_eq!(nominal, before);
    }

    #[test]
    fn nominal_inference_requires_matching_declaration_kind_and_arity() {
        let declaration = DefinitionId {
            module: ModuleIdentity::single_file("generic.kgr"),
            path: vec![DefinitionPathSegment {
                kind: DefinitionKind::Struct,
                name: "Box".into(),
                occurrence: 0,
            }],
        };
        let parameter = GenericParameterType {
            owner: declaration.clone(),
            position: 0,
            name: "T".into(),
        };
        for make in [TypeId::Struct, TypeId::Enum, TypeId::Trait] {
            let template = make(NominalType {
                declaration: declaration.clone(),
                arguments: vec![TypeId::Array(Box::new(TypeId::Generic(parameter.clone())))],
            });
            let actual = make(NominalType {
                declaration: declaration.clone(),
                arguments: vec![TypeId::Array(Box::new(TypeId::Builtin(BuiltinType::I32)))],
            });
            let mut substitution = TypeSubstitution::new();
            infer(
                &template,
                &actual,
                std::slice::from_ref(&parameter),
                &mut substitution,
                &Default::default(),
            )
            .unwrap();
            assert_eq!(template.instantiate(&substitution), actual);
            assert_eq!(substitution.len(), 1);
            let mut foreign = declaration.clone();
            foreign.module = ModuleIdentity::single_file("other.kgr");
            let wrong_kind = match &actual {
                TypeId::Struct(ty) => TypeId::Enum(ty.clone()),
                TypeId::Enum(ty) | TypeId::Trait(ty) => TypeId::Struct(ty.clone()),
                _ => unreachable!(),
            };
            for mismatch in [
                make(NominalType {
                    declaration: foreign,
                    arguments: vec![TypeId::Array(Box::new(TypeId::Builtin(BuiltinType::I32)))],
                }),
                make(NominalType {
                    declaration: declaration.clone(),
                    arguments: Vec::new(),
                }),
                wrong_kind,
            ] {
                substitution.clear();
                infer(
                    &template,
                    &mismatch,
                    std::slice::from_ref(&parameter),
                    &mut substitution,
                    &Default::default(),
                )
                .unwrap();
                assert!(substitution.is_empty());
            }
        }
    }
}
