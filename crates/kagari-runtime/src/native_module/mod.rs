//! Expansion support for `#[native_module]`; registered ABI records remain authoritative.
//! The public surface here is used by generated code in embedding consumers.
pub mod types;

use crate::{
    error::RuntimeError,
    native::{
        api::{NativeApi, NativeHandler},
        factory::NativeFactory,
    },
    native_module::types::{Scope, TypeExpression},
};
use kagari_abi::{
    callable::CallableImplementation,
    native_api::{NativeImplementation, NativeModule},
    native_import::binding_id,
    types::{
        AbiType, AssociatedTypeAbi, FunctionAbi, NominalAbiType, ParameterAbi, TraitAbi, TypeAbi,
        TypeAbiKind, native::NativeTypeConstructor,
    },
};

use kagari_common::identity::{
    DefinitionId, DefinitionKind, ModuleIdentity, PackageId, associated_type_id,
};
use std::collections::{BTreeMap, HashSet};

#[doc(hidden)]
pub struct Method {
    pub name: &'static str,
    pub documentation: &'static str,
    pub params: Vec<(&'static str, TypeExpression)>,
    pub result: TypeExpression,
    pub binding: Option<Binding>,
}

#[doc(hidden)]
pub struct Binding {
    pub name: &'static str,
    pub factory: fn() -> NativeFactory,
}

#[doc(hidden)]
pub struct AssociatedType {
    pub name: &'static str,
    pub documentation: &'static str,
}

/// A builder used by macro expansions, not a second declaration validator.
#[doc(hidden)]
pub struct NativeModuleBuilder {
    module: NativeModule,
    handlers: BTreeMap<DefinitionId, NativeHandler>,
}

impl NativeModuleBuilder {
    pub fn new(path: &[&str]) -> Result<Self, RuntimeError> {
        if path.len() < 2 {
            return Err(invalid("native module needs a package and module path"));
        }
        Ok(Self {
            module: NativeModule::new(ModuleIdentity {
                package: PackageId(
                    if path[0] == "std" {
                        "kagari-std"
                    } else {
                        path[0]
                    }
                    .into(),
                ),
                path: path[1..].iter().map(|name| (*name).into()).collect(),
            }),
            handlers: BTreeMap::new(),
        })
    }

    pub fn array_type(
        &mut self,
        name: &str,
        names: &[&'static str],
        doc: &str,
    ) -> Result<(), RuntimeError> {
        self.check_names(names)?;
        if names.len() != 1 {
            return Err(invalid("native array storage requires one type parameter"));
        }
        let owner = self.module.definition(DefinitionKind::AssociatedType, name);
        let scope = Scope {
            module: &self.module,
            owner: owner.clone(),
            names,
            receiver: None,
            associated: &[],
        };
        self.module.types.push(TypeAbi {
            name: name.into(),
            kind: TypeAbiKind::Native(NativeTypeConstructor::Array),
            generic_params: scope.generics(),
            bounds: vec![],
            fields: vec![],
            variants: vec![],
        });
        self.document(owner, doc);
        Ok(())
    }

    pub fn required_trait(
        &mut self,
        name: &str,
        names: &[&'static str],
        doc: &str,
        parents: Vec<TypeExpression>,
        associated: Vec<AssociatedType>,
        methods: Vec<Method>,
    ) -> Result<(), RuntimeError> {
        self.check_names(names)?;
        let owner = self.module.definition(DefinitionKind::Trait, name);
        let associated_names = associated
            .iter()
            .map(|member| member.name)
            .collect::<Vec<_>>();
        let scope = Scope {
            module: &self.module,
            owner: owner.clone(),
            names,
            receiver: None,
            associated: &associated_names,
        };
        let supertraits = parents
            .iter()
            .map(|parent| nominal(scope.resolve(parent)?))
            .collect::<Result<_, _>>()?;
        let functions = methods
            .iter()
            .map(|method| {
                if method.binding.is_some() {
                    return Err(invalid("required trait methods cannot bind a handler"));
                }
                function(&scope, method, CallableImplementation::Required, false)
            })
            .collect::<Result<_, _>>()?;
        self.module.traits.push(TraitAbi {
            name: name.into(),
            generic_params: scope.generics(),
            bounds: vec![],
            supertraits,
            associated_types: associated
                .iter()
                .map(|member| AssociatedTypeAbi {
                    declaration: associated_type_id(&owner, member.name),
                    generic_params: vec![],
                    parameter_bounds: vec![],
                    bounds: vec![],
                })
                .collect(),
            associated_consts: vec![],
            methods: functions,
        });
        self.document(owner.clone(), doc);
        for member in associated {
            self.document(
                associated_type_id(&owner, member.name),
                member.documentation,
            );
        }
        for method in methods {
            self.document(
                NativeModule::method_id(&owner, method.name),
                method.documentation,
            );
        }
        Ok(())
    }

    pub fn inherent_impl(
        &mut self,
        names: &[&'static str],
        receiver: TypeExpression,
        methods: Vec<Method>,
    ) -> Result<(), RuntimeError> {
        self.check_names(names)?;
        let owner = self
            .module
            .implementation_id(self.module.implementations.len());
        let for_type = Scope {
            module: &self.module,
            owner: owner.clone(),
            names,
            receiver: None,
            associated: &[],
        }
        .resolve(&receiver)?;
        let mut functions = vec![];
        for method in &methods {
            let binding = self.bind(
                method
                    .binding
                    .as_ref()
                    .ok_or_else(|| invalid("native method requires a handler"))?,
            )?;
            let scope = Scope {
                module: &self.module,
                owner: owner.clone(),
                names,
                receiver: Some(&for_type),
                associated: &[],
            };
            functions.push(function(
                &scope,
                method,
                CallableImplementation::Native(binding),
                true,
            )?);
        }
        let scope = Scope {
            module: &self.module,
            owner: owner.clone(),
            names,
            receiver: None,
            associated: &[],
        };
        self.module.implementations.push(NativeImplementation {
            generic_params: scope.generics(),
            trait_type: None,
            for_type,
            methods: functions,
        });
        for method in methods {
            self.document(
                NativeModule::method_id(&owner, method.name),
                method.documentation,
            );
        }
        Ok(())
    }

    pub fn trait_impl(
        &mut self,
        names: &[&'static str],
        receiver: TypeExpression,
        contract: TypeExpression,
        associated: Vec<(&'static str, TypeExpression)>,
        bindings: Vec<(&'static str, Binding)>,
    ) -> Result<(), RuntimeError> {
        self.check_names(names)?;
        let owner = self
            .module
            .implementation_id(self.module.implementations.len());
        let scope = Scope {
            module: &self.module,
            owner,
            names,
            receiver: None,
            associated: &[],
        };
        let for_type = scope.resolve(&receiver)?;
        let mut trait_type = nominal(scope.resolve(&contract)?)?;
        for (name, value) in associated {
            if trait_type
                .associated_types
                .insert(
                    associated_type_id(&trait_type.declaration, name),
                    scope.resolve(&value)?,
                )
                .is_some()
            {
                return Err(invalid("duplicate native associated binding"));
            }
        }
        let generics = scope.generics();
        let entries = bindings
            .iter()
            .map(|(name, binding)| Ok((*name, self.bind(binding)?)))
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        self.module
            .implement_trait(trait_type, for_type, generics, &entries)
            .map_err(|error| invalid(error.to_string()))
    }

    pub fn free_function(
        &mut self,
        names: &[&'static str],
        method: Method,
    ) -> Result<(), RuntimeError> {
        self.check_names(names)?;
        let owner = self
            .module
            .definition(DefinitionKind::Function, method.name);
        let binding = self.bind(
            method
                .binding
                .as_ref()
                .ok_or_else(|| invalid("native function requires a handler"))?,
        )?;
        let scope = Scope {
            module: &self.module,
            owner: owner.clone(),
            names,
            receiver: None,
            associated: &[],
        };
        let function = function(
            &scope,
            &method,
            CallableImplementation::Native(binding),
            true,
        )?;
        self.module.functions.push(function);
        self.document(owner, method.documentation);
        Ok(())
    }

    pub fn finish(self) -> Result<NativeApi, RuntimeError> {
        NativeApi::new(vec![self.module], self.handlers.into_values().collect())
    }

    fn bind(&mut self, binding: &Binding) -> Result<DefinitionId, RuntimeError> {
        let name = binding.name;
        if name.is_empty()
            || !name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            || name.as_bytes()[0].is_ascii_digit()
        {
            return Err(invalid("native binding name must be an identifier"));
        }
        let id = binding_id(&self.module.identity, name);
        if self.handlers.contains_key(&id) {
            return Err(invalid(
                "native binding name belongs to more than one Rust implementation",
            ));
        }
        self.handlers.insert(
            id.clone(),
            NativeHandler::from_factory(id.clone(), (binding.factory)()),
        );
        Ok(id)
    }

    fn document(&mut self, id: DefinitionId, doc: &str) {
        if !doc.is_empty() {
            self.module.documentation.insert(id, doc.into());
        }
    }

    fn check_names(&self, names: &[&str]) -> Result<(), RuntimeError> {
        let mut distinct = HashSet::new();
        if names.iter().any(|name| !distinct.insert(*name)) {
            return Err(invalid("duplicate native type parameter"));
        }
        Ok(())
    }
}

fn function(
    scope: &Scope<'_>,
    method: &Method,
    implementation: CallableImplementation,
    include_generics: bool,
) -> Result<FunctionAbi, RuntimeError> {
    Ok(FunctionAbi {
        name: method.name.into(),
        implementation,
        method_policy: Default::default(),
        generic_params: if include_generics {
            scope.generics()
        } else {
            vec![]
        },
        bounds: vec![],
        params: method
            .params
            .iter()
            .map(|(name, ty)| {
                Ok(ParameterAbi {
                    name: (*name).into(),
                    ty: scope.resolve(ty)?,
                    mutable: false,
                })
            })
            .collect::<Result<_, RuntimeError>>()?,
        return_type: scope.resolve(&method.result)?,
    })
}

fn nominal(ty: AbiType) -> Result<NominalAbiType, RuntimeError> {
    if let AbiType::Trait(nominal) = ty {
        Ok(nominal)
    } else {
        Err(invalid("expected a native trait reference"))
    }
}

fn invalid(message: impl Into<String>) -> RuntimeError {
    RuntimeError::metadata_conflict(message)
}
