//! Shared application registration for source, offline and fixture-generator targets.
use kagari_abi::{
    callable::CallableImplementation,
    native_api::NativeModule,
    native_import::binding_id,
    types::{
        ConstraintAbi, FunctionAbi, GenericBoundAbi, GenericParameterAbi, NominalAbiType,
        ParameterAbi, TraitAbi,
    },
};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, PackageId};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        NativeAction, NativeContext, NativeInvocationState,
        api::{NativeApi, NativeHandler},
        array_api::array,
        catalog::NativeCatalog,
        cmp_api::cmp,
        ops_api::ops,
        option_api::option,
        result_api::result,
        string_api::string,
    },
    value::Value,
};
use std::{cell::Cell, rc::Rc};

/// Install only the actual providers needed by the fixture's checked declarations.
pub fn dependencies() -> Result<NativeApi, RuntimeError> {
    let cmp = cmp::native_api()?;
    let ops = ops::native_api()?;
    let array = array::native_api(&NativeCatalog::from_apis(&[&ops, &cmp])?)?;
    NativeApi::combine(vec![
        cmp,
        ops,
        array,
        option::native_api()?,
        result::native_api()?,
        string::native_api()?,
    ])
}

pub fn module() -> NativeModule {
    let mut module = NativeModule::new(ModuleIdentity {
        package: PackageId("game".into()),
        path: vec!["bounds".into()],
    });
    for (name, generic_count) in [("Container", 1), ("Marker", 0)] {
        let owner = module.definition(DefinitionKind::Trait, name);
        module.traits.push(TraitAbi {
            name: name.into(),
            generic_params: (0..generic_count)
                .map(|position| GenericParameterAbi {
                    owner: owner.clone(),
                    position,
                })
                .collect(),
            bounds: vec![],
            supertraits: vec![],
            associated_types: vec![],
            associated_consts: vec![],
            methods: vec![],
        });
    }
    let owner = module.definition(DefinitionKind::Function, "echo");
    let parameters: Vec<_> = (0..2)
        .map(|position| GenericParameterAbi {
            owner: owner.clone(),
            position,
        })
        .collect();
    let constraints = ["Container", "Marker"].map(|name| {
        ConstraintAbi::Trait(NominalAbiType {
            declaration: module.definition(DefinitionKind::Trait, name),
            arguments: if name == "Container" {
                vec![parameters[1].as_type()]
            } else {
                vec![]
            },
            associated_types: Default::default(),
        })
    });
    module.functions.push(FunctionAbi {
        name: "echo".into(),
        method_policy: Default::default(),
        implementation: CallableImplementation::Native(binding_id(&module.identity, "echo")),
        bounds: vec![GenericBoundAbi {
            ty: parameters[0].as_type(),
            constraints: constraints.into(),
        }],
        params: vec![
            ParameterAbi {
                name: "value".into(),
                ty: parameters[0].as_type(),
                mutable: false,
            },
            ParameterAbi {
                name: "witness".into(),
                ty: parameters[1].as_type(),
                mutable: false,
            },
        ],
        return_type: parameters[0].as_type(),
        generic_params: parameters,
    });
    module.documentation.insert(
        owner,
        "Preserve a value under both declared trait bounds.".into(),
    );
    module
}

struct Echo(Value);
impl NativeInvocationState for Echo {
    fn advance(&mut self, _: &mut NativeContext<'_>) -> Result<NativeAction, RuntimeError> {
        Ok(NativeAction::Complete(self.0.clone()))
    }
}

pub fn api(module: NativeModule, calls: Rc<Cell<usize>>) -> NativeApi {
    let binding = binding_id(&module.identity, "echo");
    NativeApi::new(
        vec![module],
        vec![NativeHandler::new(binding, 0, move |context| {
            calls.set(calls.get() + 1);
            Ok(Box::new(Echo(context.argument(0).ok_or_else(|| {
                RuntimeError::module_validation("missing bound native input")
            })?)))
        })],
        Default::default(),
    )
    .unwrap()
}
