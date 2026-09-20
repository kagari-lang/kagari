use crate::types::{GenericParameterType, TypeId, TypeSubstitution};

/// Infer only the callee's parameters. Repeated occurrences are checked against
/// the resulting signature by the caller; no source spelling participates.
pub(super) fn infer(
    expected: &TypeId,
    actual: &TypeId,
    parameters: &[GenericParameterType],
    substitution: &mut TypeSubstitution,
) {
    if matches!(actual, TypeId::Unknown | TypeId::Error) {
        return;
    }
    match (expected, actual) {
        (TypeId::Struct(expected), TypeId::Struct(actual))
        | (TypeId::Enum(expected), TypeId::Enum(actual))
        | (TypeId::Trait(expected), TypeId::Trait(actual))
            if expected.declaration == actual.declaration
                && expected.arguments.len() == actual.arguments.len() =>
        {
            for (expected, actual) in expected.arguments.iter().zip(&actual.arguments) {
                infer(expected, actual, parameters, substitution);
            }
        }
        (TypeId::Generic(parameter), _) if parameters.contains(parameter) => {
            substitution
                .entry(parameter.clone())
                .and_modify(|inferred| inferred.recover_from(actual))
                .or_insert_with(|| actual.clone());
        }
        (TypeId::Tuple(expected), TypeId::Tuple(actual)) if expected.len() == actual.len() => {
            for (expected, actual) in expected.iter().zip(actual) {
                infer(expected, actual, parameters, substitution);
            }
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
            for (expected, actual) in expected.iter().zip(actual) {
                infer(expected, actual, parameters, substitution);
            }
        }
        (TypeId::Array(expected), TypeId::Array(actual))
        | (TypeId::Set(expected), TypeId::Set(actual)) => {
            infer(expected, actual, parameters, substitution)
        }
        (TypeId::Map { key: ek, value: ev }, TypeId::Map { key: ak, value: av }) => {
            infer(ek, ak, parameters, substitution);
            infer(ev, av, parameters, substitution);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BuiltinType, NominalType};
    use kagari_common::identity::{
        DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity,
    };

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
            );
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
                );
                assert!(substitution.is_empty());
            }
        }
    }
}
