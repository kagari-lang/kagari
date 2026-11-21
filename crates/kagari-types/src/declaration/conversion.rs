//! Installed forward conversion metadata and admission of library forwarding impls.
use crate::{
    declaration::{TraitDef, script::ScriptExpr},
    language::Protocol,
    ty::{GenericBound, GenericParam, NominalTy, Ty},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::{DefinitionId, DefinitionTable},
    },
};

/// A checked forward method/result contract. Ordinary implementations select the error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionAdapter {
    pub method: DefinitionId,
    pub error: DefinitionId,
    pub result: DefinitionId,
}

impl RemapDefinitions for ConversionAdapter {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(Self {
            method: mapper.reference(&self.method)?,
            error: mapper.reference(&self.error)?,
            result: mapper.reference(&self.result)?,
        })
    }
}

impl DefinitionReferences for ConversionAdapter {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.method)?;
        visit(&self.error)?;
        visit(&self.result)
    }
}

impl ConversionAdapter {
    /// Validate the adapter's local declaration shape before linked proofs use it.
    pub fn valid_in(
        &self,
        owner: &DefinitionId,
        contract: &TraitDef,
        definitions: &DefinitionTable,
    ) -> bool {
        let ([parameter], [method], [error]) = (
            contract.generic_params.as_slice(),
            contract.methods.as_slice(),
            contract.associated_types.as_slice(),
        ) else {
            return false;
        };
        if !contract.associated_consts.is_empty()
            || method.name.is_empty()
            || definitions.lookup_child(*owner, DefinitionKind::Method, &method.name, 0)
                != Some(self.method)
            || error.declaration != self.error
            || !error.generic_params.is_empty()
        {
            return false;
        }
        let input = parameter.as_type();
        let this = Ty::SelfType(*owner);
        let output = Ty::Enum(NominalTy {
            declaration: self.result,
            arguments: vec![
                this.clone(),
                Ty::Projection {
                    receiver: Box::new(this),
                    interface: Box::new(NominalTy {
                        declaration: *owner,
                        arguments: vec![input.clone()],
                        associated_types: Default::default(),
                    }),
                    member: self.error,
                    arguments: vec![],
                },
            ],
            associated_types: Default::default(),
        });
        method.generic_params.is_empty()
            && method.params.len() == 1
            && method.params[0].ty == input
            && method.return_type == output
    }
}

/// Recognize the scoped `core::convert::{Into, TryInto}` identities, not their spellings.
/// This identifies reserved impl headers; it does not provide an implementation proof.
pub fn is_library_forwarding_trait(definitions: &DefinitionTable, id: DefinitionId) -> bool {
    let Some(root) = definitions.lookup_root(&Protocol::From.module()) else {
        return false;
    };
    ["Into", "TryInto"]
        .iter()
        .any(|name| definitions.lookup_child(root, DefinitionKind::Trait, name, 0) == Some(id))
}

/// Required expression of a validated library blanket. The destination bound,
/// not executable metadata, selects the forward operation.
pub fn forwarding_expression(
    definitions: &DefinitionTable,
    bound: &GenericBound,
    fallible: bool,
) -> Option<ScriptExpr> {
    let [forward] = bound.constraints.as_slice() else {
        return None;
    };
    let target = if fallible { "try_from" } else { "from" };
    Some(ScriptExpr::Call {
        callee: Box::new(ScriptExpr::TraitMethod {
            receiver: bound.ty.clone(),
            interface: forward.clone(),
            member: definitions.lookup_child(
                forward.declaration,
                DefinitionKind::Method,
                target,
                0,
            )?,
        }),
        arguments: vec![ScriptExpr::Name("self".into())],
        type_arguments: vec![],
    })
}

/// Check a reserved forwarding header. Only the library's own two-binder impl with
/// the exact destination bound (and, for `TryInto`, projected `Error`) qualifies.
/// Unrelated traits return false; their implementations use ordinary admission.
pub fn library_forwarding_header_valid(
    definitions: &DefinitionTable,
    owner: DefinitionId,
    parameters: &[GenericParam],
    receiver: &Ty,
    interface: &NominalTy,
    bounds: &[GenericBound],
) -> bool {
    let Some(root) = definitions.lookup_root(&Protocol::From.module()) else {
        return false;
    };
    let library_trait = |name| definitions.lookup_child(root, DefinitionKind::Trait, name, 0);
    let fallible = library_trait("TryInto") == Some(interface.declaration);
    if !fallible && library_trait("Into") != Some(interface.declaration) {
        return false;
    }
    let Some(forward) = library_trait(if fallible { "TryFrom" } else { "From" }) else {
        return false;
    };
    let [source, destination] = parameters else {
        return false;
    };
    let [bound] = bounds else { return false };
    let [required] = bound.constraints.as_slice() else {
        return false;
    };
    if definitions.parent(owner) != Ok(Some(root))
        || !definitions.resolve(owner).is_ok_and(|view| {
            view.last()
                .is_some_and(|part| part.kind == DefinitionKind::Impl && part.name.is_empty())
        })
        || source.owner != owner
        || source.position != 0
        || destination.owner != owner
        || destination.position != 1
        || receiver != &source.as_type()
        || interface.arguments.as_slice() != [destination.as_type()]
        || bound.ty != destination.as_type()
        || required.declaration != forward
        || required.arguments.as_slice() != [source.as_type()]
        || !required.associated_types.is_empty()
    {
        return false;
    }
    if !fallible {
        return interface.associated_types.is_empty();
    }
    let error = |id| definitions.lookup_child(id, DefinitionKind::AssociatedType, "Error", 0);
    let (Some(output), Some(member)) = (error(interface.declaration), error(forward)) else {
        return false;
    };
    interface.associated_types.len() == 1
        && interface.associated_types.get(&output)
            == Some(&Ty::Projection {
                receiver: Box::new(destination.as_type()),
                interface: Box::new(required.clone()),
                member,
                arguments: vec![],
            })
}
