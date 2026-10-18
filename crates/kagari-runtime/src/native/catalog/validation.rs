//! Semantic module checks shared by declaration sealing and native preparation.
//! These operations inspect complete contracts only; they never prepare adapters.
use crate::{
    error::RuntimeError,
    native::{binding::NativeResult, catalog::DeclarationCatalog},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, table::DefinitionId},
};
use kagari_types::declaration::{
    NativeDeclaration, applications::ApplicationValidator, module::ModuleDecl,
};

impl DeclarationCatalog {
    /// Validate local shape and ownership in this catalog's scope. The module may
    /// supply local constructor records not yet inserted into the catalog.
    pub(crate) fn validate_module_shape(&self, module: &ModuleDecl) -> NativeResult<()> {
        let definitions = self.definitions();
        let owners = self.receiver_owners(Some(module))?;
        module
            .validate(&|ty| owners.owner(ty, &definitions), &definitions)
            .map_err(conflict)
    }

    /// Validate declarations against the complete available catalog and return
    /// their native projection. Binding coverage, storage adapters and linked
    /// implementation/default proofs remain separate checks at their owners.
    pub(crate) fn validate_module_declarations(
        &self,
        module: &ModuleDecl,
    ) -> NativeResult<Vec<NativeDeclaration>> {
        self.validate_module_shape(module)?;
        for target in module.exports.values() {
            self.checked_export_owner(*target)?;
        }
        let cancel = CancellationToken::default();
        let applications = ApplicationValidator::new(
            &cancel,
            |id| self.traits.get_id(*id),
            |id| {
                self.types
                    .get_id(*id)
                    .map(|ty| (ty.kind, ty.generic_params.len()))
            },
        );
        for ty in &module.types {
            applications
                .bounds(&ty.bounds)
                .map_err(|error| conflict(format!("type applications: {error:?}")))?;
            applications
                .types(
                    ty.fields
                        .iter()
                        .map(|field| &field.ty)
                        .chain(ty.variants.iter().flat_map(|variant| &variant.payload)),
                )
                .map_err(|error| conflict(format!("type applications: {error:?}")))?;
        }
        for contract in &module.traits {
            applications
                .trait_definition(contract)
                .map_err(|error| conflict(format!("trait applications: {error:?}")))?;
        }
        for implementation in &module.implementations {
            applications
                .validate_type(&implementation.for_type)
                .map_err(|error| conflict(format!("receiver application: {error:?}")))?;
            applications
                .bounds(&implementation.bounds)
                .map_err(|error| conflict(format!("implementation bounds: {error:?}")))?;
            if let Some(interface) = &implementation.trait_type {
                applications
                    .trait_application(interface)
                    .map_err(|error| conflict(format!("implementation application: {error:?}")))?;
            }
        }
        let declarations = module
            .native_declarations(&self.definitions())
            .map_err(conflict)?;
        for declaration in &declarations {
            applications
                .function(&declaration.function)
                .map_err(|error| conflict(format!("function application: {error:?}")))?;
            if let Some(ty) = &declaration.concrete_result {
                applications
                    .validate_type(ty)
                    .map_err(|error| conflict(format!("concrete result application: {error:?}")))?;
            }
        }
        Ok(declarations)
    }

    /// Resolve an exported type/trait or enum variant to its declared owner.
    /// Interning a path alone is not evidence that the export exists.
    pub(crate) fn checked_export_owner(&self, target: DefinitionId) -> NativeResult<DefinitionId> {
        let definitions = self.definitions();
        let target_view = definitions.resolve(target).map_err(conflict)?;
        let owner = if target_view
            .last()
            .is_some_and(|part| part.kind == DefinitionKind::Variant)
        {
            definitions
                .parent(target)
                .map_err(conflict)?
                .ok_or_else(|| conflict("export variant has no owner"))?
        } else {
            target
        };
        let owner_view = definitions.resolve(owner).map_err(conflict)?;
        let exists = match owner_view.last().map(|part| part.kind) {
            Some(DefinitionKind::Trait) => self.traits.get_id(target).is_some(),
            Some(
                DefinitionKind::Struct | DefinitionKind::Enum | DefinitionKind::AssociatedType,
            ) => self.types.get_id(owner).is_some_and(|ty| {
                owner == target
                    || target_view.last().is_some_and(|member| {
                        ty.variants
                            .iter()
                            .any(|variant| variant.name == member.name)
                    })
            }),
            _ => false,
        };
        if !exists {
            return Err(conflict(
                "re-export target is not declared by an available provider",
            ));
        }
        Ok(owner)
    }
}

fn conflict(error: impl ToString) -> RuntimeError {
    RuntimeError::metadata_conflict(error.to_string())
}
