//! Binding identities and contracts reserved by the core Runtime, not by a product.
use crate::{
    native_import::NativeImport,
    types::{PublicItem, type_contract},
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity,
    table::{DefinitionId, DefinitionTable},
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::NativeDeclaration,
    language::{binding, schema::CoreEnum},
    range::RangeKind,
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy, Ty},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoundationNative {
    RangeBound(RangeKind),
    MakeErrResidual,
    ForwardErrOrigin,
}

impl FoundationNative {
    /// Match a scoped free-function identity, never a source expression spelling.
    pub fn for_id(id: DefinitionId, definitions: &DefinitionTable) -> Option<Self> {
        let definition = definitions.resolve(id).ok()?;
        let mut segments = definition.segments();
        let segment = segments.next()?;
        if segments.next().is_some()
            || definition.module().package.0 != "kagari-core"
            || segment.kind != DefinitionKind::Function
            || segment.occurrence != 0
        {
            return None;
        }
        match definition.module().path.as_slice() {
            [module] if module == "ops" => Some(Self::RangeBound(match segment.name {
                "range_bound" => RangeKind::Exclusive,
                "inclusive_bound" => RangeKind::Inclusive,
                "from_bound" => RangeKind::From,
                "to_bound" => RangeKind::To,
                "to_inclusive_bound" => RangeKind::ToInclusive,
                "full_bound" => RangeKind::Full,
                _ => return None,
            })),
            [module] if module == "result" => match segment.name {
                "make_err_residual" => Some(Self::MakeErrResidual),
                "forward_err_origin" => Some(Self::ForwardErrOrigin),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn import_allowed(
        import: &NativeImport,
        caller: &ModuleIdentity,
        definitions: &DefinitionTable,
    ) -> bool {
        let declaration = import.instance.declaration;
        if Self::for_id(declaration, definitions).is_none()
            && Self::for_id(import.binding, definitions).is_none()
        {
            return true;
        }
        declaration == import.binding
            && definitions
                .resolve(declaration)
                .is_ok_and(|view| view.module() == caller)
    }

    /// Independently specify the checked ABI of the implementation. The carried
    /// declaration is evidence to compare, not a way to choose what Rust accepts.
    pub fn declaration_matches(
        self,
        declaration: &NativeDeclaration,
        definitions: &DefinitionTable,
    ) -> bool {
        let id = declaration.declaration;
        if Self::for_id(id, definitions) != Some(self) {
            return false;
        }
        let Some((arity, signature)) = self.signature(id, definitions) else {
            return false;
        };
        let function = &declaration.function;
        function.implementation == CallableImplementation::Native(id)
            && definitions
                .resolve(id)
                .is_ok_and(|view| view.last().is_some_and(|part| part.name == function.name))
            && function
                .generic_params
                .iter()
                .map(|parameter| (parameter.owner, parameter.position))
                .eq((0..arity).map(|position| (id, position)))
            && function.bounds.is_empty()
            && function.host_policy.is_none()
            && function.method_policy == Default::default()
            && function.params.iter().all(|parameter| !parameter.mutable)
            && function
                .params
                .iter()
                .map(|parameter| &parameter.ty)
                .eq(&signature.params)
            && function.return_type == signature.result
            && declaration.concrete_result.is_none()
            && declaration.callable_requirements.is_empty()
    }

    pub fn schema_matches(
        self,
        owner: &ModuleIdentity,
        items: &[PublicItem],
        definitions: &DefinitionTable,
    ) -> bool {
        if matches!(self, Self::RangeBound(_)) {
            return true;
        }
        let Some(id) = definitions.lookup(&binding::result_declaration()) else {
            return false;
        };
        type_contract(owner, items, &id, definitions)
            .is_some_and(|schema| CoreEnum::Result.shape_valid(id, schema, definitions))
    }

    fn signature(
        self,
        id: DefinitionId,
        definitions: &DefinitionTable,
    ) -> Option<(usize, Signature)> {
        let parameter = |position| {
            GenericParam {
                owner: id,
                position,
            }
            .as_type()
        };
        let never = Ty::Builtin(BuiltinType::Never);
        let result = |value, error| binding::result(definitions, value, error);
        let (arity, params, result) = match self {
            Self::RangeBound(kind) => {
                let item = parameter(0);
                let range_item = if kind == RangeKind::Full {
                    Ty::Builtin(BuiltinType::Unit)
                } else {
                    item.clone()
                };
                let bound = Ty::Enum(NominalTy {
                    declaration: definitions.lookup(&binding::bound_declaration())?,
                    arguments: vec![item],
                    associated_types: Default::default(),
                });
                (
                    1,
                    vec![
                        Ty::Range(Box::new(range_item), kind),
                        Ty::Builtin(BuiltinType::Bool),
                    ],
                    bound,
                )
            }
            Self::MakeErrResidual => {
                let residual = result(never, parameter(1))?;
                (
                    2,
                    vec![result(parameter(0), parameter(1))?, residual.clone()],
                    residual,
                )
            }
            Self::ForwardErrOrigin => {
                let converted = result(parameter(0), parameter(2))?;
                (
                    3,
                    vec![result(never, parameter(1))?, converted.clone()],
                    converted,
                )
            }
        };
        Some((arity, Signature { params, result }))
    }
}
