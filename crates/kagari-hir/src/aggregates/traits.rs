use super::*;
use crate::{typeck::ConstraintTarget, types::GenericParameterType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodParameter {
    pub name: String,
    pub writeability: Writeability,
    pub ty: TypeId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodSignature {
    pub id: DefinitionId,
    pub owner: DefinitionId,
    pub slot: usize,
    pub name: String,
    pub generic_params: Vec<GenericParameterType>,
    pub bounds: Vec<(GenericParameterType, Vec<ConstraintTarget>)>,
    pub params: Vec<MethodParameter>,
    pub return_type: TypeId,
    pub declaration: Declaration,
}

impl MethodSignature {
    fn same_contract(&self, other: &Self) -> bool {
        self.id == other.id
            && self.owner == other.owner
            && self.slot == other.slot
            && self.name == other.name
            && self.generic_params == other.generic_params
            && self.bounds == other.bounds
            && self.params == other.params
            && self.return_type == other.return_type
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitSignature {
    pub id: DefinitionId,
    pub generic_params: Vec<GenericParameterType>,
    pub methods: Vec<MethodSignature>,
    pub declaration: Declaration,
}

impl AggregateCatalog {
    pub fn traits(&self) -> impl Iterator<Item = &TraitSignature> {
        self.traits.values().map(AsRef::as_ref)
    }

    pub fn trait_(&self, id: &DefinitionId) -> Option<&TraitSignature> {
        self.traits.get(id).map(AsRef::as_ref)
    }

    pub fn trait_method(&self, id: &DefinitionId) -> Option<&MethodSignature> {
        let (owner, slot) = self.methods.get(id)?;
        self.trait_(owner)?.methods.get(*slot)
    }

    pub(super) fn add_traits(
        &mut self,
        lowered: &LoweredModule,
        declarations: &Declarations,
        signatures: &ModuleSignatures,
        cancel: &CancellationToken,
    ) -> Result<(), Cancelled> {
        let mut functions = std::collections::HashMap::new();
        for function in signatures.functions() {
            cancel.check()?;
            functions.insert(function.id, function);
        }
        for item in &lowered.module.traits {
            cancel.check()?;
            let declaration = declarations
                .target(ResolvedName::Trait(item.id))
                .expect("trait declaration");
            let DeclarationId::Definition(id) = &declaration.id else {
                unreachable!("nominal trait");
            };
            let mut generic_params = Vec::new();
            for param in &item.generic_params {
                cancel.check()?;
                generic_params.push(
                    declarations
                        .generic_type(param.id)
                        .expect("trait parameter identity"),
                );
            }
            let mut methods = Vec::new();
            for (slot, method) in item.methods.iter().enumerate() {
                cancel.check()?;
                let declaration = declarations
                    .target(ResolvedName::Function(method.function))
                    .expect("method declaration");
                let DeclarationId::Definition(method_id) = &declaration.id else {
                    unreachable!("nominal method");
                };
                let function = functions
                    .get(&method.function)
                    .expect("checked method signature");
                let source = lowered
                    .module
                    .functions
                    .iter()
                    .find(|f| f.id == method.function)
                    .expect("method generic binder");
                let mut bounds = crate::typeck::function_bounds(
                    &lowered.module,
                    source,
                    declarations,
                    signatures.type_table(),
                );
                let mut params = Vec::new();
                for param in &function.params {
                    cancel.check()?;
                    params.push(MethodParameter {
                        name: param.name.clone(),
                        writeability: param.writeability,
                        ty: param.ty.clone(),
                    });
                }
                self.methods
                    .insert(method_id.clone(), (id.clone(), methods.len()));
                methods.push(MethodSignature {
                    id: method_id.clone(),
                    owner: id.clone(),
                    slot,
                    name: method.name.clone(),
                    bounds: function
                        .generic_params
                        .iter()
                        .map(|p| (p.clone(), bounds.remove(p).unwrap_or_default()))
                        .collect(),
                    generic_params: function.generic_params.clone(),
                    params,
                    return_type: function.return_type.clone(),
                    declaration: declaration.clone(),
                });
            }
            self.traits.insert(
                id.clone(),
                Arc::new(TraitSignature {
                    id: id.clone(),
                    generic_params,
                    methods,
                    declaration: declaration.clone(),
                }),
            );
        }
        Ok(())
    }

    pub(super) fn include_traits(
        &self,
        result: &mut Self,
        module: &ModuleIdentity,
        cancel: &CancellationToken,
    ) -> Result<(), Cancelled> {
        let start = DefinitionId {
            module: module.clone(),
            path: Vec::new(),
        };
        for (id, item) in self
            .traits
            .range(start..)
            .take_while(|(id, _)| &id.module == module)
        {
            cancel.check()?;
            for (index, method) in item.methods.iter().enumerate() {
                cancel.check()?;
                result
                    .methods
                    .insert(method.id.clone(), (id.clone(), index));
            }
            result.traits.insert(id.clone(), item.clone());
        }
        Ok(())
    }

    pub(super) fn same_trait_contracts(&self, other: &Self) -> bool {
        self.traits.len() == other.traits.len()
            && self.traits.iter().all(|(id, a)| {
                other.trait_(id).is_some_and(|b| {
                    a.generic_params == b.generic_params
                        && a.methods.len() == b.methods.len()
                        && a.methods
                            .iter()
                            .zip(&b.methods)
                            .all(|(a, b)| a.same_contract(b))
                })
            })
    }
}
