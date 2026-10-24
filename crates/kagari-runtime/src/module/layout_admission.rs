//! Compatibility evidence belongs to immutable type descriptions, not live values.
//! Weak producer links cannot retain executable instances or create descriptor cycles.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    frame::types::compatibility::TypeView,
    module::{
        EnumVariantRef, ExecutableModule, InstallationId, ModuleKey, ProgramDescriptor,
        StructLayoutRef, descriptor_index::DescriptorIndex, layout_identity::LayoutIdentity,
    },
};
use std::sync::{Arc, Mutex, OnceLock, Weak};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum AggregateKind {
    Struct,
    Enum,
}

#[derive(Clone, Copy)]
pub(super) struct LayoutEndpoint<'a> {
    pub(super) owner: &'a ExecutableModule,
    pub(super) identity: Option<LayoutIdentity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct AdmissionKey {
    installation: InstallationId,
    producer: ModuleKey,
    actual: LayoutIdentity,
    expected: LayoutIdentity,
}

type AdmissionIndex = DescriptorIndex<AggregateKind, AdmissionKey, Weak<ProgramDescriptor>>;

/// Complete nominal meaning without retaining a layout's argument bindings.
/// The immutable program does not own type arguments; this proof cannot create
/// a cycle through a supplied argument's lexical environment.
#[derive(Debug)]
pub(crate) struct NominalAdmission {
    owner: ExecutableModule,
    identity: LayoutIdentity,
    kind: AggregateKind,
}

impl NominalAdmission {
    pub(crate) fn structure(layout: &StructLayoutRef) -> Option<Self> {
        Some(Self {
            owner: layout.module.clone(),
            identity: layout.canonical?,
            kind: AggregateKind::Struct,
        })
    }

    pub(crate) fn enumeration(layout: &EnumVariantRef) -> Option<Self> {
        Some(Self {
            owner: layout.module.clone(),
            identity: layout.canonical?,
            kind: AggregateKind::Enum,
        })
    }

    pub(crate) fn matches_struct(&self, actual: &StructLayoutRef, expected: TypeView<'_>) -> bool {
        self.kind == AggregateKind::Struct
            && admit(
                self.kind,
                LayoutEndpoint {
                    owner: &self.owner,
                    identity: Some(self.identity),
                },
                LayoutEndpoint {
                    owner: &actual.module,
                    identity: actual.canonical,
                },
                || actual.matches_view(expected),
            )
    }

    pub(crate) fn matches_enum(&self, actual: &EnumVariantRef, expected: TypeView<'_>) -> bool {
        // Type admission accepts every valid member; pattern admission separately
        // checks the selected tag through EnumVariantRef::matches_layout.
        self.kind == AggregateKind::Enum
            && admit(
                self.kind,
                LayoutEndpoint {
                    owner: &self.owner,
                    identity: Some(self.identity),
                },
                LayoutEndpoint {
                    owner: &actual.module,
                    identity: actual.canonical,
                },
                || actual.matches_view(expected),
            )
    }
}

/// Lazily allocated, bounded pure-fact evidence; safe to share with detached type facts.
#[derive(Debug, Default)]
pub(super) struct LayoutAdmissions {
    entries: OnceLock<Box<Mutex<AdmissionIndex>>>,
}

impl LayoutAdmissions {
    fn contains(
        &self,
        kind: AggregateKind,
        key: &AdmissionKey,
        producer: &ExecutableModule,
    ) -> bool {
        self.entries
            .get()
            .and_then(|entries| {
                let entries = entries.try_lock().ok()?;
                entries.get(&kind, key)?.upgrade()
            })
            .is_some_and(|owner| Arc::ptr_eq(&owner, &producer.program))
    }

    fn retain(&self, kind: AggregateKind, key: AdmissionKey, producer: &ExecutableModule) {
        let entries = self
            .entries
            .get_or_init(|| Box::new(Mutex::new(Default::default())));
        if let Ok(mut entries) = entries.try_lock() {
            // Contention, duplicate publication or capacity failure loses reuse only.
            // No graph comparison, callback or executable borrow crosses this lock.
            let _ = entries.insert(kind, key, Arc::downgrade(&producer.program));
        }
    }
}

/// Admit a complete layout pair once while preserving all per-value checks at callers.
/// Program identity and never-recycled layout IDs name immutable scoped meanings.
pub(super) fn admit(
    kind: AggregateKind,
    expected: LayoutEndpoint<'_>,
    actual: LayoutEndpoint<'_>,
    compatible: impl FnOnce() -> bool,
) -> bool {
    if expected.owner.registry_owner != actual.owner.registry_owner
        || expected.owner.installation_id() != actual.owner.installation_id()
    {
        return false;
    }
    let identities = expected.identity.zip(actual.identity);
    if let Some((expected_id, actual_id)) = identities
        && Arc::ptr_eq(&expected.owner.program, &actual.owner.program)
        && expected_id == actual_id
    {
        return true;
    }
    let key = identities.map(|(expected_id, actual_id)| AdmissionKey {
        installation: actual.owner.installation_id(),
        producer: actual.owner.program_key(),
        actual: actual_id,
        expected: expected_id,
    });
    if let Some(key) = &key
        && expected
            .owner
            .program
            .layout_admissions
            .contains(kind, key, actual.owner)
    {
        return true;
    }
    #[cfg(feature = "execution-diagnostics")]
    diagnostics::record(Event::LayoutComparison);
    if !compatible() {
        return false;
    }
    if let Some(key) = key {
        expected
            .owner
            .program
            .layout_admissions
            .retain(kind, key, actual.owner);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Runtime,
        frame::types::bindings::TypeBindings,
        test_support::{layouts as layout_fixtures, native::program::ProgramFixture},
        value::{EnumTag, Value},
        value_check,
    };
    use kagari_bytecode::{
        instruction::{EnumId, StructId},
        module::BytecodeModule,
        program::{BytecodeProgram, ModuleRef},
    };
    use kagari_common::identity::{
        DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata,
    };
    use kagari_contract::layout::{EnumLayout, EnumVariantLayout, StructLayout};
    use kagari_types::{
        scalar::BuiltinType,
        ty::{GenericParam, NominalTy, Ty},
    };
    use std::slice;

    fn installation_layouts() -> DefinitionMetadata<BytecodeProgram> {
        let context = DefinitionContext::new().unwrap();
        let mut module = BytecodeModule::default();
        let root = context.intern_root(&module.identity).unwrap();
        let record = context
            .intern_child(root, DefinitionKind::Struct, "Record", 0)
            .unwrap();
        let holder = context
            .intern_child(root, DefinitionKind::Struct, "Holder", 0)
            .unwrap();
        let choice = context
            .intern_child(root, DefinitionKind::Enum, "Choice", 0)
            .unwrap();
        module.structures = vec![
            StructLayout {
                declaration: record,
                arguments: vec![],
                fields: vec![],
            },
            StructLayout {
                declaration: holder,
                arguments: vec![Ty::Parameter {
                    owner: holder,
                    position: 0,
                }],
                fields: vec![],
            },
        ];
        module.enumerations = vec![EnumLayout {
            declaration: choice,
            arguments: vec![Ty::Parameter {
                owner: choice,
                position: 0,
            }],
            variants: vec![EnumVariantLayout {
                declaration: context
                    .intern_child(choice, DefinitionKind::Variant, "Some", 0)
                    .unwrap(),
                payload: vec![Ty::Parameter {
                    owner: choice,
                    position: 0,
                }],
                reports_failure: false,
            }],
        }];
        ProgramFixture {
            root: ModuleRef::new(0),
            modules: vec![module],
            exports: Default::default(),
        }
        .metadata(context.snapshot(), &Default::default())
        .unwrap()
    }

    #[test]
    fn installation_nominals_reject_cached_fallback_and_nested_lexical_admission() {
        let mut runtime = Runtime::default();
        let native_owner = layout_fixtures::sequence_owner(&mut runtime);
        let native_root = native_owner
            .definitions()
            .lookup_root(&native_owner.bytecode.identity)
            .unwrap();
        let native_declaration = native_owner
            .definitions()
            .lookup_child(native_root, DefinitionKind::AssociatedType, "Vec", 0)
            .unwrap();
        let code = installation_layouts();
        let left_program = runtime.load_program("same", code.clone()).unwrap();
        let left = runtime.select_program(&left_program).unwrap();
        let right_program = runtime.load_program("same", code.clone()).unwrap();
        let right = runtime.select_program(&right_program).unwrap();
        let record = left.struct_layout(StructId::new(0)).unwrap();
        let foreign_record = right.struct_layout(StructId::new(0)).unwrap();
        let nominal = record.type_expression();
        let argument = runtime
            .resolve_type_arguments(&left, slice::from_ref(&nominal))
            .unwrap()
            .remove(0);
        let foreign = runtime
            .resolve_type_arguments(&right, slice::from_ref(&nominal))
            .unwrap()
            .remove(0);
        assert_ne!(argument.identity(&left), foreign.identity(&right));
        assert!(!record.matches(&foreign_record));
        assert!(
            !TypeView::new(&nominal, &left, None).compatible(TypeView::new(&nominal, &right, None))
        );
        let admission = NominalAdmission::structure(&record).unwrap();
        assert!(!admission.matches_struct(&foreign_record, argument.view(&left)));
        let value = Value::Struct(runtime.alloc_struct(foreign_record, vec![]).unwrap());
        assert!(!argument.matches(&runtime, &value, &left));
        assert!(!value_check::matches_view(
            runtime.gc(),
            &value,
            argument.view(&left)
        ));
        assert!(runtime.prepare_struct_layout(&left, &foreign).is_err());
        assert!(
            runtime
                .prepare_struct_application(&left, StructId::new(1), slice::from_ref(&foreign))
                .is_err()
        );
        let holder = left.bytecode.structures[1].declaration;
        // Borrowed storage disables optional scope/application caches, not admission.
        let held = runtime.modules.inner.borrow_mut();
        let detached = runtime
            .prepare_struct_application(&left, StructId::new(1), slice::from_ref(&argument))
            .unwrap();
        let detached_foreign = runtime
            .prepare_struct_application(&right, StructId::new(1), slice::from_ref(&foreign))
            .unwrap();
        assert!(!detached.matches(&detached_foreign));
        drop(held);
        let foreign_scope = runtime
            .prepare_layout_scope(&right, holder, slice::from_ref(&foreign))
            .unwrap();
        assert!(
            runtime
                .modules
                .applied_struct_layout(
                    &left,
                    StructId::new(1),
                    slice::from_ref(&nominal),
                    foreign_scope
                )
                .is_none()
        );

        let parameter = GenericParam {
            owner: holder,
            position: 0,
        };
        let bindings = Arc::new(
            TypeBindings::new(
                runtime.definition_context(),
                vec![parameter.clone()],
                vec![foreign],
            )
            .unwrap(),
        );
        let nested = Ty::Tuple(vec![Ty::Array(Box::new(parameter.as_type()))]);
        assert!(!TypeView::new(&nested, &left, Some(&bindings)).belongs_to_installation(&left));
        assert!(
            runtime
                .type_arguments(&left, Some(bindings), &[nested])
                .is_err()
        );

        let scalar = Ty::Builtin(BuiltinType::I32);
        let enumeration = Ty::Enum(NominalTy {
            declaration: left.bytecode.enumerations[0].declaration,
            arguments: vec![scalar.clone()],
            associated_types: Default::default(),
        });
        let enum_argument = runtime
            .resolve_type_arguments(&left, slice::from_ref(&enumeration))
            .unwrap()
            .remove(0);
        let foreign_enum_argument = runtime
            .resolve_type_arguments(&right, slice::from_ref(&enumeration))
            .unwrap()
            .remove(0);
        let variant = runtime
            .prepare_enum_layout(&left, &enum_argument)
            .unwrap()
            .unwrap();
        let foreign_variant = runtime
            .prepare_enum_layout(&right, &foreign_enum_argument)
            .unwrap()
            .unwrap();
        assert!(!variant.matches_layout(&foreign_variant));
        let admission = NominalAdmission::enumeration(&variant).unwrap();
        assert!(!admission.matches_enum(&foreign_variant, enum_argument.view(&left)));
        let value = Value::Enum(
            runtime
                .alloc_enum(EnumTag::Declared(foreign_variant), vec![Value::I32(7)])
                .unwrap(),
        );
        assert!(!enum_argument.matches(&runtime, &value, &left));
        assert!(!value_check::matches_view(
            runtime.gc(),
            &value,
            enum_argument.view(&left)
        ));
        assert!(
            runtime
                .prepare_enum_layout(&left, &foreign_enum_argument)
                .is_err()
        );
        let nested_enum = Ty::Array(Box::new(enumeration));
        assert!(
            !TypeView::new(&nested_enum, &left, None).compatible(TypeView::new(
                &nested_enum,
                &right,
                None
            ))
        );
        assert!(
            TypeView::new(&scalar, &left, None).compatible(TypeView::new(&scalar, &right, None))
        );
        let native = Ty::NativeObject(NominalTy {
            declaration: native_declaration,
            arguments: vec![scalar.clone()],
            associated_types: Default::default(),
        });
        let native_argument = runtime
            .resolve_type_arguments(&right, slice::from_ref(&native))
            .unwrap()
            .remove(0);
        assert!(
            TypeView::new(&native, &left, None).compatible(TypeView::new(&native, &right, None))
        );
        assert!(
            runtime
                .prepare_struct_application(&left, StructId::new(1), &[native_argument])
                .is_ok()
        );
        let native_nominal = Ty::NativeObject(NominalTy {
            declaration: native_declaration,
            arguments: vec![nominal.clone()],
            associated_types: Default::default(),
        });
        let native_foreign = runtime
            .resolve_type_arguments(&right, &[native_nominal])
            .unwrap()
            .remove(0);
        assert!(
            runtime
                .prepare_struct_application(&left, StructId::new(1), &[native_foreign])
                .is_err()
        );
        let scalar_argument = runtime
            .resolve_type_arguments(&right, &[scalar])
            .unwrap()
            .remove(0);
        assert!(
            runtime
                .prepare_struct_application(&left, StructId::new(1), &[scalar_argument])
                .is_ok()
        );

        let staged = runtime
            .stage_reload_program(&left_program, None, code)
            .unwrap();
        let current = staged.module().clone();
        runtime.publish_staged_reload(staged).unwrap();
        let current_record = current.struct_layout(StructId::new(0)).unwrap();
        assert_eq!(left.installation_id(), current.installation_id());
        assert!(record.matches(&current_record));
        assert!(record.matches(&current_record)); // Retained admission cache.
        let fresh = runtime
            .resolve_type_arguments(&current, slice::from_ref(&nominal))
            .unwrap()
            .remove(0);
        assert_ne!(argument.identity(&left), fresh.identity(&current));
        assert!(argument.view(&left).compatible(fresh.view(&current)));
        let retained_application = runtime
            .prepare_struct_application(&current, StructId::new(1), slice::from_ref(&argument))
            .unwrap();
        let fresh_application = runtime
            .prepare_struct_application(&current, StructId::new(1), &[fresh])
            .unwrap();
        assert!(detached.matches(&retained_application));
        assert!(detached.matches(&fresh_application));
        assert!(!detached_foreign.matches(&fresh_application));
        let current_enum = current
            .applied_enum_variant(EnumId::new(0), &[Ty::Builtin(BuiltinType::I32)], 0)
            .unwrap();
        assert!(variant.matches_layout(&current_enum));
        assert!(variant.matches_layout(&current_enum));
        let current_value = Value::Enum(
            runtime
                .alloc_enum(EnumTag::Declared(current_enum), vec![Value::I32(7)])
                .unwrap(),
        );
        assert!(enum_argument.matches(&runtime, &current_value, &current));
        assert!(!foreign_enum_argument.matches(&runtime, &current_value, &right));
    }
}
