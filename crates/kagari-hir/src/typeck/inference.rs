//! Infer callee generic substitutions by structurally matching semantic types.
//! This is separate from body-local solver variables; the caller validates the
//! applied signature and repeated occurrences after collecting substitutions.

use crate::{
    aggregates::{AggregateCatalog, implementations::ImplementationSearchLimits},
    typeck::{GenericBounds, members},
    types::{GenericParameterType, NominalType, TypeId, TypeSubstitution},
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::table::DefinitionTable,
};

#[cfg(test)]
use kagari_types::collection::CollectionAccess;

/// Available implementation evidence at the inference site. Header matching
/// passes None: finding a substitution does not authorize receiver conversion.
#[derive(Clone, Copy)]
pub(crate) struct Context<'a> {
    pub definitions: &'a DefinitionTable,
    pub aggregates: &'a AggregateCatalog,
    pub assumptions: &'a GenericBounds,
}

/// Infer only the callee's parameters. Repeated occurrences are checked against
/// the resulting signature by the caller; no source spelling participates.
pub(crate) fn infer(
    expected: &TypeId,
    actual: &TypeId,
    parameters: &[GenericParameterType],
    substitution: &mut TypeSubstitution,
    cancel: &CancellationToken,
    context: Option<Context<'_>>,
) -> Result<(), Cancelled> {
    if let (Some(context), TypeId::Trait(expected_interface)) = (context, expected)
        && !matches!(actual, TypeId::Trait(actual) if expected_interface.declaration == actual.declaration)
    {
        return context.infer_argument(
            expected_interface,
            actual,
            parameters,
            substitution,
            cancel,
        );
    }
    if let (Some(context), TypeId::Trait(target)) = (context, actual)
        && matches!(
            expected,
            TypeId::Struct(_)
                | TypeId::NativeObject(_)
                | TypeId::Enum(_)
                | TypeId::Array(_)
                | TypeId::Map { .. }
                | TypeId::Set(_, _)
        )
    {
        return context.infer_result(expected, target, parameters, substitution, cancel);
    }
    let mut pending = vec![(expected, actual)];
    while let Some((expected, actual)) = pending.pop() {
        cancel.check()?;
        if matches!(actual, TypeId::Unknown | TypeId::Error) {
            continue;
        }
        match (expected, actual) {
            (
                TypeId::Reference {
                    target: expected,
                    access: ea,
                },
                TypeId::Reference {
                    target: actual,
                    access: aa,
                },
            ) if ea == aa => {
                pending.push((expected, actual));
            }
            (TypeId::NativeObject(expected), TypeId::NativeObject(actual))
            | (TypeId::Struct(expected), TypeId::Struct(actual))
            | (TypeId::Enum(expected), TypeId::Enum(actual))
            | (TypeId::Trait(expected), TypeId::Trait(actual))
                if expected.declaration == actual.declaration
                    && expected.arguments.len() == actual.arguments.len() =>
            {
                pending.extend(expected.arguments.iter().zip(&actual.arguments).rev());
                for (member, expected) in &expected.associated_types {
                    if let Some(actual) = actual.associated_types.get(member) {
                        pending.push((expected, actual));
                    }
                }
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

            (TypeId::Range(expected, a), TypeId::Range(actual, b)) if a == b => {
                pending.push((expected, actual));
            }
            (TypeId::Iter(expected), TypeId::Iter(actual))
            | (TypeId::Array(expected), TypeId::Array(actual))
            | (TypeId::Set(expected, _), TypeId::Set(actual, _)) => {
                pending.push((expected, actual));
            }
            (
                TypeId::Map {
                    key: ek, value: ev, ..
                },
                TypeId::Map {
                    key: ak, value: av, ..
                },
            ) => {
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

impl Context<'_> {
    fn infer_argument(
        self,
        expected_interface: &NominalType,
        actual: &TypeId,
        parameters: &[GenericParameterType],
        substitution: &mut TypeSubstitution,
        cancel: &CancellationToken,
    ) -> Result<(), Cancelled> {
        let mut selected = None;
        for available in members::interfaces(
            self.definitions,
            self.aggregates,
            actual,
            self.assumptions,
            cancel,
        ) {
            cancel.check()?;
            if available.declaration != expected_interface.declaration {
                continue;
            }
            let mut candidate = substitution.clone();
            infer(
                &TypeId::Trait(expected_interface.clone()),
                &TypeId::Trait(available.clone()),
                parameters,
                &mut candidate,
                cancel,
                None,
            )?;
            let applied = expected_interface.instantiate(&candidate);
            if applied.arguments != available.arguments
                || !applied
                    .associated_types
                    .iter()
                    .all(|(id, ty)| available.associated_types.get(id) == Some(ty))
            {
                continue;
            }
            if selected
                .as_ref()
                .is_some_and(|previous| previous != &candidate)
            {
                // Preserve the unresolved input for the caller's inference diagnostic.
                return Ok(());
            }
            selected = Some(candidate);
        }
        cancel.check()?;
        if let Some(selected) = selected {
            *substitution = selected;
        }
        Ok(())
    }

    fn infer_result(
        self,
        expected: &TypeId,
        target: &NominalType,
        parameters: &[GenericParameterType],
        substitution: &mut TypeSubstitution,
        cancel: &CancellationToken,
    ) -> Result<(), Cancelled> {
        let mut selected = None;
        for implementation in self.aggregates.implementations() {
            cancel.check()?;
            if implementation.trait_type.declaration != target.declaration {
                continue;
            }
            let mut receiver_arguments = TypeSubstitution::default();
            infer(
                &implementation.for_type,
                expected,
                &implementation.generic_params,
                &mut receiver_arguments,
                cancel,
                None,
            )?;
            let header = implementation.trait_type.instantiate(&receiver_arguments);
            let mut candidate = substitution.clone();
            infer(
                &TypeId::Trait(header),
                &TypeId::Trait(target.clone()),
                parameters,
                &mut candidate,
                cancel,
                None,
            )?;
            // Contextual return inference can start with unknown receiver arguments.
            // Discharge the implementation bounds only after solving those slots.
            let receiver = expected.instantiate(&candidate);
            if !matches!(
                self.aggregates.concrete_interface_implementation(
                    self.definitions,
                    target,
                    &receiver,
                    self.assumptions,
                    ImplementationSearchLimits {
                        max_checks: 100_000,
                        max_depth: 64
                    },
                    cancel,
                ),
                Ok(Some(_))
            ) {
                continue;
            }
            if selected
                .as_ref()
                .is_some_and(|previous| previous != &candidate)
            {
                return Ok(());
            }
            selected = Some(candidate);
        }
        cancel.check()?;
        if let Some(selected) = selected {
            *substitution = selected;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{tests::test_analysis, types::NominalType};
    use kagari_common::identity::{
        DefinitionKind, ModuleIdentity,
        map::DefinitionContext,
        table::{DefinitionId, DefinitionTable},
    };
    use kagari_source::{
        diagnostic::DiagnosticKind, source::SourceFile, source_database::SourceSnapshot,
    };
    use kagari_stdlib::{catalog, identity as library};
    use kagari_types::scalar::BuiltinType;
    use std::sync::Arc;

    fn definition(
        context: &DefinitionContext,
        module: &str,
        kind: DefinitionKind,
        name: &str,
    ) -> DefinitionId {
        let root = context
            .intern_root(&ModuleIdentity::single_file(module))
            .unwrap();
        context.intern_child(root, kind, name, 0).unwrap()
    }

    fn foundation_interface(definitions: &DefinitionTable, name: &str) -> NominalType {
        NominalType {
            declaration: definitions.lookup(&library::trait_id(name)).unwrap(),
            arguments: vec![],
            associated_types: Default::default(),
        }
    }

    #[test]
    fn collection_context_infers_slots_across_native_and_declared_views() {
        let source = Arc::new(SourceFile::new("memory://views.kgr", "fn consume<T>() {}"));
        let id = source.id();
        let snapshot = test_analysis()
            .snapshot(SourceSnapshot::single_file(source), &Default::default())
            .unwrap();
        let facts = snapshot
            .file(id)
            .unwrap()
            .to_unverified(&Default::default())
            .unwrap();
        let definitions = facts.definitions();
        let parameter = GenericParameterType {
            owner: definitions
                .lookup_child(
                    definitions
                        .lookup_root(facts.records().facts().lowered.source.module_identity())
                        .unwrap(),
                    DefinitionKind::Function,
                    "consume",
                    0,
                )
                .unwrap(),
            position: 0,
            name: "T".into(),
        };
        let mut expected = foundation_interface(definitions, "Set");
        expected.arguments = vec![TypeId::Generic(parameter.clone())];
        let integer = TypeId::Builtin(BuiltinType::I32);
        let mut writable = foundation_interface(definitions, "MutableSet");
        writable.arguments = vec![integer.clone()];
        for actual in [
            TypeId::Set(Box::new(integer.clone()), CollectionAccess::Mutable),
            TypeId::Trait(writable),
        ] {
            let mut substitution = TypeSubstitution::default();
            infer(
                &TypeId::Trait(expected.clone()),
                &actual,
                std::slice::from_ref(&parameter),
                &mut substitution,
                &Default::default(),
                Some(Context {
                    definitions,
                    aggregates: &facts.records().facts().aggregates,
                    assumptions: &Default::default(),
                }),
            )
            .unwrap();
            assert_eq!(substitution[&parameter], integer);
        }
    }

    #[test]
    fn interface_argument_inference_checks_bounds_and_rejects_ambiguous_applications() {
        let declarations = r#"
trait Read<T> { fn read(self) -> T; }
struct Cell<T> { val value: T }
impl<T: Eq> Read<T> for Cell<T> { fn read(self) -> T { self.value } }
struct Both {}
impl Read<i32> for Both { fn read(self) -> i32 { 42 } }
impl Read<i64> for Both { fn read(self) -> i64 { 42i64 } }
fn take<T>(value: Read<T>) -> T { value.read() }
fn identity<T>(value: T) -> T { value }
fn make<T: Eq>() -> Cell<T> { loop {} }
"#;
        for (body, accepted) in [
            (
                "fn main() -> i32 { val x = Cell { value: 42 }; take(x) }",
                true,
            ),
            (
                "fn forward<T: Eq>(value: T) -> T { val x = Cell { value }; take(x) }",
                true,
            ),
            (
                "fn main() { val x = Cell { value: 1.0f32 }; take(x); }",
                false,
            ),
            (
                "fn main() -> i32 { val x = Both {}; val value: i32 = take(x); value }",
                true,
            ),
            ("fn main() { val x = Both {}; take(x); }", false),
            ("fn inferred_result() -> Read<i32> { make() }", true),
            ("fn invalid_result() -> Read<f32> { make() }", false),
            (
                "fn pass(value: Read<i32>) -> Read<i32> { identity(value) }",
                true,
            ),
        ] {
            let source =
                SourceFile::new("interface-inference.kgr", format!("{declarations}\n{body}"));
            let analysis = crate::analyze_source(&source, catalog::shared().unwrap()).unwrap();
            if accepted {
                assert!(
                    analysis.diagnostics().is_empty(),
                    "{body}: {:?}",
                    analysis.diagnostics()
                );
            } else {
                assert!(
                    analysis.diagnostics().iter().any(|diagnostic| matches!(
                        diagnostic.kind,
                        DiagnosticKind::CannotInferGenericArgument { .. }
                    )),
                    "{body}: {:?}",
                    analysis.diagnostics()
                );
            }
        }
    }

    #[test]
    fn deep_inference_is_iterative_cancellable_and_preserves_member_order() {
        let context = DefinitionContext::new().unwrap();
        let parameter = GenericParameterType {
            owner: definition(&context, "deep.kgr", DefinitionKind::Function, "infer"),
            position: 0,
            name: "T".into(),
        };
        let mut expected = TypeId::Generic(parameter.clone());
        let mut actual = TypeId::Builtin(BuiltinType::I32);
        for _ in 0..10_000 {
            expected = TypeId::Array(Box::new(expected));
            actual = TypeId::Array(Box::new(actual));
        }
        let mut substitution = TypeSubstitution::default();
        let cancelled = kagari_common::cancellation::CancellationToken::default();
        cancelled.cancel();
        infer(
            &expected,
            &actual,
            std::slice::from_ref(&parameter),
            &mut substitution,
            &cancelled,
            None,
        )
        .unwrap_err();
        assert!(substitution.is_empty());
        infer(
            &expected,
            &actual,
            std::slice::from_ref(&parameter),
            &mut substitution,
            &Default::default(),
            None,
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
            None,
        )
        .unwrap();
        assert_eq!(substitution[&parameter], TypeId::Builtin(BuiltinType::I32));
    }

    #[test]
    fn deep_recovery_and_conflicts_use_iterative_member_walks() {
        let mut recovering: TypeId = TypeId::Error;
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
        let mut value: TypeId = TypeId::Map {
            key: Box::new(TypeId::Builtin(BuiltinType::I32)),
            value: Box::new(TypeId::Tuple(vec![
                TypeId::Error,
                TypeId::Builtin(BuiltinType::Bool),
            ])),
            access: CollectionAccess::Mutable,
        };
        let other = TypeId::Map {
            key: Box::new(TypeId::Builtin(BuiltinType::Bool)),
            value: Box::new(TypeId::Tuple(vec![
                TypeId::Builtin(BuiltinType::I32),
                TypeId::Builtin(BuiltinType::I32),
            ])),
            access: CollectionAccess::Mutable,
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
                access: CollectionAccess::Mutable
            }
        );
        assert!(value.conflicts_with(&other));
        let mut different_arity: TypeId = TypeId::Tuple(vec![TypeId::Error]);
        different_arity.recover_from(&TypeId::Tuple(vec![TypeId::Builtin(BuiltinType::I32); 2]));
        assert_eq!(different_arity, TypeId::Tuple(vec![TypeId::Error]));
        let context = DefinitionContext::new().unwrap();
        let declaration = definition(&context, "a.kgr", DefinitionKind::Struct, "Box");
        let mut nominal = TypeId::Struct(NominalType {
            associated_types: Default::default(),
            declaration,
            arguments: vec![TypeId::Error],
        });
        let before = nominal.clone();
        nominal.recover_from(&TypeId::Enum(NominalType {
            associated_types: Default::default(),
            declaration,
            arguments: vec![TypeId::Builtin(BuiltinType::I32)],
        }));
        assert_eq!(nominal, before);
        let foreign = definition(&context, "b.kgr", DefinitionKind::Struct, "Box");
        nominal.recover_from(&TypeId::Struct(NominalType {
            associated_types: Default::default(),
            declaration: foreign,
            arguments: vec![TypeId::Builtin(BuiltinType::I32)],
        }));
        assert_eq!(nominal, before);
    }

    #[test]
    fn nominal_inference_requires_matching_declaration_kind_and_arity() {
        let context = DefinitionContext::new().unwrap();
        let declaration = definition(&context, "generic.kgr", DefinitionKind::Struct, "Box");
        let parameter = GenericParameterType {
            owner: declaration,
            position: 0,
            name: "T".into(),
        };
        for make in [
            TypeId::NativeObject,
            TypeId::Struct,
            TypeId::Enum,
            TypeId::Trait,
        ] {
            let template = make(NominalType {
                associated_types: Default::default(),
                declaration,
                arguments: vec![TypeId::Array(Box::new(TypeId::Generic(parameter.clone())))],
            });
            let actual = make(NominalType {
                associated_types: Default::default(),
                declaration,
                arguments: vec![TypeId::Array(Box::new(TypeId::Builtin(BuiltinType::I32)))],
            });
            let mut substitution = TypeSubstitution::default();
            infer(
                &template,
                &actual,
                std::slice::from_ref(&parameter),
                &mut substitution,
                &Default::default(),
                None,
            )
            .unwrap();
            assert_eq!(template.instantiate(&substitution), actual);
            assert_eq!(substitution.len(), 1);
            let foreign = definition(&context, "other.kgr", DefinitionKind::Struct, "Box");
            let wrong_kind = match &actual {
                TypeId::Struct(ty) => TypeId::Enum(ty.clone()),
                TypeId::NativeObject(ty) | TypeId::Enum(ty) | TypeId::Trait(ty) => {
                    TypeId::Struct(ty.clone())
                }
                _ => unreachable!(),
            };
            for mismatch in [
                make(NominalType {
                    associated_types: Default::default(),
                    declaration: foreign,
                    arguments: vec![TypeId::Array(Box::new(TypeId::Builtin(BuiltinType::I32)))],
                }),
                make(NominalType {
                    associated_types: Default::default(),
                    declaration,
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
                    None,
                )
                .unwrap();
                assert!(substitution.is_empty());
            }
        }
    }
}
