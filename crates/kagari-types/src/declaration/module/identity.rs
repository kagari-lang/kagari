//! Identity registration belongs to construction; semantic queries borrow a table.
use crate::declaration::module::{DeclarationError, ModuleDecl};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        map::DefinitionContext,
        scoped::DefinitionReferences,
        table::{DefinitionId, DefinitionTable},
    },
};

fn absent() -> DeclarationError {
    DeclarationError("missing registered native identity".into())
}

impl ModuleDecl {
    /// Register implicit declaration names after checking the complete input scope.
    /// This collects identities only; it grants no declaration or binding evidence.
    pub fn collect_definitions(
        &self,
        context: &DefinitionContext,
    ) -> Result<DefinitionTable, DeclarationError> {
        if !self.declaration_count_valid() {
            return Err(DeclarationError("too many native declarations".into()));
        }
        let invalid = |error| DeclarationError(format!("invalid native identities: {error}"));
        let prefix = context.snapshot();
        self.visit_definition_ids(
            &mut |id| Ok(prefix.resolve(*id).map(|_| ())?),
            &CancellationToken::default(),
        )
        .map_err(|error| DeclarationError(format!("invalid native identities: {error}")))?;
        let root = context.intern_root(&self.identity).map_err(invalid)?;
        for ty in &self.types {
            let owner = context
                .intern_child(root, ty.kind.definition_kind(), &ty.name, 0)
                .map_err(invalid)?;
            for field in &ty.fields {
                context
                    .intern_child(owner, DefinitionKind::Field, &field.name, 0)
                    .map_err(invalid)?;
            }
            for variant in &ty.variants {
                context
                    .intern_child(owner, DefinitionKind::Variant, &variant.name, 0)
                    .map_err(invalid)?;
            }
        }
        for contract in &self.traits {
            let owner = context
                .intern_child(root, DefinitionKind::Trait, &contract.name, 0)
                .map_err(invalid)?;
            for method in &contract.methods {
                context
                    .intern_child(owner, DefinitionKind::Method, &method.name, 0)
                    .map_err(invalid)?;
            }
        }
        for (owner, contract) in &self.trait_references {
            for method in &contract.methods {
                context
                    .intern_child(*owner, DefinitionKind::Method, &method.name, 0)
                    .map_err(invalid)?;
            }
        }
        for function in &self.functions {
            context
                .intern_child(root, DefinitionKind::Function, &function.name, 0)
                .map_err(invalid)?;
        }
        for (index, implementation) in self.implementations.iter().enumerate() {
            let occurrence = u32::try_from(index)
                .map_err(|_| DeclarationError("too many native implementations".into()))?;
            let owner = context
                .intern_child(root, DefinitionKind::Impl, "", occurrence)
                .map_err(invalid)?;
            for method in &implementation.methods {
                context
                    .intern_child(owner, DefinitionKind::Method, &method.name, 0)
                    .map_err(invalid)?;
            }
            if let Some(interface) = &implementation.trait_type {
                for member in interface.associated_types.keys() {
                    let member = prefix.resolve(*member).map_err(invalid)?;
                    let name = member.last().ok_or_else(absent)?.name;
                    context
                        .intern_child(owner, DefinitionKind::AssociatedType, name, 0)
                        .map_err(invalid)?;
                }
            }
        }
        Ok(context.snapshot())
    }

    pub fn definition(
        &self,
        kind: DefinitionKind,
        name: &str,
        definitions: &DefinitionTable,
    ) -> Result<DefinitionId, DeclarationError> {
        let root = definitions.lookup_root(&self.identity).ok_or_else(absent)?;
        definitions
            .lookup_child(root, kind, name, 0)
            .ok_or_else(absent)
    }

    pub fn implementation_id(
        &self,
        index: usize,
        definitions: &DefinitionTable,
    ) -> Result<DefinitionId, DeclarationError> {
        let occurrence = u32::try_from(index)
            .map_err(|_| DeclarationError("too many native implementations".into()))?;
        let root = definitions.lookup_root(&self.identity).ok_or_else(absent)?;
        definitions
            .lookup_child(root, DefinitionKind::Impl, "", occurrence)
            .ok_or_else(absent)
    }

    pub fn method_id(
        owner: DefinitionId,
        name: &str,
        definitions: &DefinitionTable,
    ) -> Result<DefinitionId, DeclarationError> {
        definitions
            .lookup_child(owner, DefinitionKind::Method, name, 0)
            .ok_or_else(absent)
    }

    pub fn variant_id(
        owner: DefinitionId,
        name: &str,
        definitions: &DefinitionTable,
    ) -> Result<DefinitionId, DeclarationError> {
        definitions
            .lookup_child(owner, DefinitionKind::Variant, name, 0)
            .ok_or_else(absent)
    }
}
