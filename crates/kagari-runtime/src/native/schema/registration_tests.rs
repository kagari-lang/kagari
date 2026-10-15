//! Manual registration failures must roll back unpublished schemas.
use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::KagariType,
        schema::{TypeRegistration, ValueField},
        types::Type,
    },
};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext};
use kagari_types::ty::{NominalTy, Ty};

#[path = "../../../tests/support/schema_leaf.rs"]
mod leaf;
use leaf::Leaf;

struct Missing;

impl KagariType for Missing {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog.rust_type::<Self>()
    }

    fn register_type(cx: &mut TypeRegistration<'_>) -> NativeResult<Type> {
        let definitions = DefinitionContext::new().unwrap();
        let root = definitions
            .intern_root(&ModuleIdentity::single_file("absent"))
            .unwrap();
        let id = definitions
            .intern_child(root, DefinitionKind::Struct, "Unknown", 0)
            .unwrap();
        let ty = Type::from_semantic(
            Ty::Struct(NominalTy {
                declaration: id,
                arguments: vec![],
                associated_types: Default::default(),
            }),
            &definitions.snapshot(),
        )?;
        cx.structure::<Self>("Missing", |_| Ok(vec![ValueField::new("bad", ty)]))
    }
}

#[test]
fn missing_schema_dependency_rejects_without_publishing_headers_or_mappings() {
    let mut builder = ModuleBuilder::new("game::atomic", &DeclarationCatalog::default()).unwrap();
    assert!(builder.register_type::<Missing>().is_err());
    assert!(builder.declarations().rust_type::<Missing>().is_err());
    builder.register_type::<Leaf>().unwrap();
    let module = builder.finish().unwrap();
    assert_eq!(module.declaration().types.len(), 1);
    assert_eq!(module.declaration().types[0].name, "Leaf");
}

struct FailedChild;

impl KagariType for FailedChild {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog.rust_type::<Self>()
    }

    fn register_type(cx: &mut TypeRegistration<'_>) -> NativeResult<Type> {
        cx.structure::<Self>("FailedChild", |cx| {
            cx.dependency::<Missing>()?;
            Err(RuntimeError::metadata_conflict("field schema failed"))
        })
    }
}

struct CatchesChild;

impl KagariType for CatchesChild {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog.rust_type::<Self>()
    }

    fn register_type(cx: &mut TypeRegistration<'_>) -> NativeResult<Type> {
        cx.structure::<Self>("CatchesChild", |cx| {
            let _ = cx.dependency::<FailedChild>();
            Ok(vec![])
        })
    }
}

#[test]
fn caught_child_schema_failure_cannot_publish_an_incomplete_dependency() {
    let mut builder = ModuleBuilder::new("game::caught", &DeclarationCatalog::default()).unwrap();
    assert!(
        builder
            .register_type::<CatchesChild>()
            .unwrap_err()
            .message()
            .contains("incomplete schema")
    );
    assert!(builder.declarations().rust_type::<CatchesChild>().is_err());
    assert!(builder.declarations().rust_type::<FailedChild>().is_err());
    builder.register_type::<Leaf>().unwrap();
    builder.finish().unwrap();
}
