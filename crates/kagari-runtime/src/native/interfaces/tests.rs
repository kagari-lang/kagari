use super::*;
use crate::{
    Runtime,
    frame::types::{bindings::TypeBindings, compatibility::TypeView},
    module::VerifiedProgram,
    native::{builder::ModuleBuilder, context::ScriptInvoker, typed::NativeContext},
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::identity::{DefinitionKind, metadata::DefinitionMetadata, table::DefinitionId};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::types::PublicItem;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy},
};
use std::{iter, slice, sync::Arc};

fn fixture() -> (Runtime, VerifiedProgram, DefinitionId, DefinitionId) {
    let mut provider = ModuleBuilder::new("authority::native", &Default::default()).unwrap();
    let mut shared = provider.define_trait("Shared").unwrap();
    shared.type_parameter("T").unwrap();
    shared.finish().unwrap();
    let provider = provider.finish().unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![Arc::new(provider.to_declaration())]);
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "interface-authority.kgr",
            r#"
        use authority::native::Shared;
        pub trait Counter { fn next(self) -> i32; }
        impl Counter for i32 { fn next(self) -> i32 { self + 1 } }
        impl Shared<i32> for i32 {}
        pub fn accept(value: Counter) -> i32 { value.next() }
        pub fn shared(value: Shared<i32>) -> Shared<i32> { value }
        pub fn make_shared() -> Shared<i32> { 7 }
        fn main() -> Counter { 7 }
    "#
            .into(),
            SourceLayer::Base,
        )
        .unwrap();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&mir, &Default::default()).unwrap();
    let artifact =
        KbcArtifact::from_verified(bytecode, Default::default(), &Default::default()).unwrap();
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    drop((artifact, mir, checked, snapshot, analysis, sources));

    let mut runtime = Runtime::default();
    provider.install(&mut runtime).unwrap();
    let adopted = KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(runtime.definition_context(), &Default::default())
        .unwrap();
    let verified = adopted
        .into_records()
        .into_verified(
            &Default::default(),
            runtime.definition_context(),
            &Default::default(),
        )
        .unwrap();
    let prepared = VerifiedProgram::from_bytecode(verified.into_bytecode());
    let definitions = prepared.definitions();
    let script = &prepared.modules()[prepared.root().index()];
    let root = definitions.lookup_root(&script.identity).unwrap();
    let counter = definitions
        .lookup_child(root, DefinitionKind::Trait, "Counter", 0)
        .unwrap();
    let shared = prepared
        .modules()
        .iter()
        .find_map(|module| {
            let root = definitions.lookup_root(&module.identity)?;
            definitions.lookup_child(root, DefinitionKind::Trait, "Shared", 0)
        })
        .unwrap();
    let contract = script
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicItem::Trait(contract) if contract.name == "Counter" => Some(contract.clone()),
            _ => None,
        })
        .unwrap();
    let path = definitions.resolve(counter).unwrap().to_path();
    let mut reference = ModuleBuilder::new("authority::reference", &provider.catalog()).unwrap();
    reference
        .reference_trait(
            &path,
            DefinitionMetadata::checked(definitions.clone(), contract, &Default::default())
                .unwrap(),
        )
        .unwrap();
    reference.finish().unwrap().install(&mut runtime).unwrap();
    // A catalog entry is not provider authority: this is an imported script requirement.
    let catalog = &runtime.native_entries.catalog;
    assert!(catalog.traits.get_id(counter).is_some());
    assert!(!catalog.contains_module(definitions.resolve(counter).unwrap().module()));
    assert!(catalog.contains_module(definitions.resolve(shared).unwrap().module()));
    // Even a provider of the same module does not own a merely referenced trait.
    let identity = &script.identity;
    let name = iter::once(identity.package.0.as_str())
        .chain(identity.path.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("::");
    ModuleBuilder::new(&name, &Default::default())
        .unwrap()
        .finish()
        .unwrap()
        .install(&mut runtime)
        .unwrap();
    assert!(runtime.native_entries.catalog.contains_module(identity));
    assert!(!runtime.native_entries.owned_traits.contains(&counter));
    assert!(runtime.native_entries.owned_traits.contains(&shared));
    (runtime, prepared, counter, shared)
}

fn trait_type(declaration: DefinitionId, arguments: Vec<Ty>) -> Ty {
    Ty::Trait(NominalTy {
        declaration,
        arguments,
        associated_types: Default::default(),
    })
}

fn interface_value(
    runtime: &Runtime,
    owner: &ExecutableModule,
    declaration: DefinitionId,
) -> Value {
    let (owner, table) = owner.members().find_map(|owner| {
        let implementation = owner.bytecode.public_items.iter().find_map(|item| match item {
            PublicItem::InterfaceTable(table) if matches!(&table.trait_type, Ty::Trait(ty) if ty.declaration == declaration) => Some(table.declaration),
            _ => None,
        })?;
        let table = owner.bytecode.interface_tables.iter().position(|table| table.declaration == implementation)?;
        Some((owner, table))
    }).unwrap();
    runtime
        .make_interface(&owner, table, Value::I32(7))
        .unwrap()
}

#[test]
fn script_interface_admission_is_installation_scoped_across_conversion_and_reload() {
    let (mut runtime, prepared, counter, _) = fixture();
    let left_program = runtime
        .load_verified_program("same", prepared.clone())
        .unwrap();
    let right_program = runtime
        .load_verified_program("same", prepared.clone())
        .unwrap();
    let left = runtime.select_program(&left_program).unwrap();
    let right = runtime.select_program(&right_program).unwrap();
    assert!(!left.is_shared_native_trait(counter));
    assert!(!right.is_shared_native_trait(counter));
    let ty = trait_type(counter, vec![]);
    let left_argument = runtime
        .resolve_type_arguments(&left, slice::from_ref(&ty))
        .unwrap()
        .remove(0);
    let right_argument = runtime
        .resolve_type_arguments(&right, slice::from_ref(&ty))
        .unwrap()
        .remove(0);
    assert!(left_argument.has_origin());
    assert_ne!(
        left_argument.identity(&left),
        right_argument.identity(&right)
    );
    assert!(
        !left_argument
            .view(&right)
            .compatible(right_argument.view(&right))
    );
    let value = interface_value(&runtime, &left, counter);
    // SAFETY: The fixture interface stays live; the built-in dynamic Interface mapping cannot
    // collect before admission.
    let handle: Interface = unsafe {
        ConversionContext::new(&runtime, &left)
            .unwrap()
            .decode_transport(&left_argument, &value)
    }
    .unwrap();
    assert!(!runtime.matches_type_in(&value, &ty, &right, None));
    assert!(
        unsafe {
            ConversionContext::new(&runtime, &right)
                .unwrap()
                .decode_transport::<Interface>(&right_argument, &value)
        }
        .is_err()
    );
    assert!(
        ConversionContext::new(&runtime, &right)
            .unwrap()
            .encode_value(&right_argument, handle.clone())
            .is_err()
    );
    let accept = runtime
        .bind_function::<(Interface,), i32>(&right_program.root(), "accept")
        .unwrap();
    let mut context = NativeContext::with_invoker(&runtime, &right, unsafe {
        // SAFETY: The callback rejects execution or returns only a scalar; it retains no raw edges.
        ScriptInvoker::new(|_, _, _, _| {
            panic!("cross-installation interface reached script execution")
        })
    })
    .unwrap();
    let error = accept.call(&mut context, (handle.clone(),)).unwrap_err();
    assert!(error.message().contains("interface view differs"));
    drop(context);

    let staged = runtime
        .stage_reload_verified_program(&left_program, None, prepared)
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();
    let current = runtime.select_program(&left_program).unwrap();
    let fresh = runtime
        .resolve_type_arguments(&current, slice::from_ref(&ty))
        .unwrap()
        .remove(0);
    assert_ne!(left_argument.identity(&left), fresh.identity(&current));
    assert!(left_argument.view(&left).compatible(fresh.view(&current)));
    assert!(runtime.matches_type_in(&value, &ty, &current, None));
    assert_eq!(
        ConversionContext::new(&runtime, &current)
            .unwrap()
            .encode_value(&fresh, handle)
            .unwrap()
            .value(&runtime)
            .unwrap(),
        value
    );
    assert!(!runtime.matches_type_in(&value, &ty, &right, None));
}

#[test]
fn shared_native_interfaces_keep_nested_script_trait_provenance() {
    let (mut runtime, prepared, counter, shared) = fixture();
    let left_program = runtime
        .load_verified_program("same", prepared.clone())
        .unwrap();
    let right_program = runtime.load_verified_program("same", prepared).unwrap();
    let left = runtime.select_program(&left_program).unwrap();
    let right = runtime.select_program(&right_program).unwrap();
    assert!(left.is_shared_native_trait(shared));
    assert!(right.is_shared_native_trait(shared));
    let scalar = Ty::Builtin(BuiltinType::I32);
    let ty = trait_type(shared, vec![scalar.clone()]);
    let left_argument = runtime
        .resolve_type_arguments(&left, slice::from_ref(&ty))
        .unwrap()
        .remove(0);
    let right_argument = runtime
        .resolve_type_arguments(&right, slice::from_ref(&ty))
        .unwrap()
        .remove(0);
    assert_eq!(
        left_argument.identity(&left),
        right_argument.identity(&right)
    );
    assert!(
        left_argument
            .view(&left)
            .compatible(right_argument.view(&right))
    );
    assert!(TypeView::new(&scalar, &left, None).compatible(TypeView::new(&scalar, &right, None)));
    let value = interface_value(&runtime, &left, shared);
    // SAFETY: The fixture interface stays live; the built-in dynamic Interface mapping cannot
    // collect before admission.
    let handle: Interface = unsafe {
        ConversionContext::new(&runtime, &left)
            .unwrap()
            .decode_transport(&left_argument, &value)
    }
    .unwrap();
    assert!(runtime.matches_type_in(&value, &ty, &right, None));
    assert_eq!(
        ConversionContext::new(&runtime, &right)
            .unwrap()
            .encode_value(&right_argument, handle)
            .unwrap()
            .value(&runtime)
            .unwrap(),
        value
    );

    let script = trait_type(counter, vec![]);
    let nested = trait_type(shared, vec![Ty::Array(Box::new(script.clone()))]);
    let left_nested = runtime
        .resolve_type_arguments(&left, slice::from_ref(&nested))
        .unwrap()
        .remove(0);
    let right_nested = runtime
        .resolve_type_arguments(&right, slice::from_ref(&nested))
        .unwrap()
        .remove(0);
    assert_ne!(left_nested.identity(&left), right_nested.identity(&right));
    assert!(
        !left_nested
            .view(&left)
            .compatible(right_nested.view(&right))
    );
    assert!(left_nested.validate_for(&runtime, &right).is_err());
    let parameter = GenericParam {
        owner: shared,
        position: 0,
    };
    let script_argument = runtime
        .resolve_type_arguments(&left, &[script])
        .unwrap()
        .remove(0);
    let bindings = Arc::new(
        TypeBindings::new(
            runtime.definition_context(),
            vec![parameter.clone()],
            vec![script_argument],
        )
        .unwrap(),
    );
    let lexical = trait_type(shared, vec![Ty::Array(Box::new(parameter.as_type()))]);
    assert!(
        runtime
            .type_arguments(&right, Some(bindings), &[lexical])
            .is_err()
    );
}
