//! Application-owned ordinary associated contracts shared by tests and generation.
use kagari_abi::{
    callable::CallableImplementation,
    native_api::{NativeImplementation, NativeModule},
    native_import::{binding_id, callables::NativeCallableRequirement},
    scalar::BuiltinType,
    types::{
        AbiType, AssociatedTypeAbi, ConstraintAbi, FunctionAbi, GenericBoundAbi,
        GenericParameterAbi, NominalAbiType, ParameterAbi, TraitAbi, TypeAbi, TypeAbiKind,
        native::NativeTypeConstructor,
    },
};
use kagari_common::{
    collection::CollectionAccess,
    identity::{DefinitionKind, ModuleIdentity, PackageId, associated_type_id},
};
use kagari_native_macros::native_module;
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        NativeAction, NativeContext, NativeInvocationState,
        api::{NativeApi, NativeHandler},
    },
    value::Value,
};
use std::{cell::Cell, rc::Rc};

#[native_module("game::typed_associated")]
pub mod typed {
    use kagari_runtime::{
        error::{RuntimeError, RuntimeErrorKind},
        native_value::{NativeResult, NativeValue, array::NativeArray},
    };
    #[native_type]
    pub struct Bag<T: NativeValue>(NativeArray<T>);
    #[native_trait]
    pub trait TypedSource {
        /// The actual Rust implementation supplies this value type.
        type Item: NativeValue;
        fn head(&self) -> NativeResult<Self::Item>;
        fn round_trip(&self, value: Option<(Self::Item,)>) -> Option<(Self::Item,)>;
        fn keep(&self, other: Self) -> Self;
    }
    #[native_trait]
    pub trait Transform<X: NativeValue> {
        type Item: NativeValue;
        fn swap(&self, item: Self::Item, other: X) -> (X, <Self as Transform<X>>::Item);
    }
    #[native_impl]
    impl<T: NativeValue> Transform<i32> for Bag<T> {
        type Item = T;
        fn swap(&self, item: Self::Item, other: i32) -> (i32, <Self as Transform<i32>>::Item) {
            (other, item)
        }
    }
    #[native_impl]
    impl<T: NativeValue> TypedSource for Bag<T> {
        type Item = T;
        fn head(&self) -> NativeResult<Self::Item> {
            self.0.get(0)?.ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::IndexOutOfBounds, "empty source")
            })
        }
        fn round_trip(&self, value: Option<(Self::Item,)>) -> Option<(Self::Item,)> {
            value
        }
        fn keep(&self, other: Self) -> Self {
            other
        }
    }
}

pub fn module() -> NativeModule {
    let mut module = NativeModule::new(ModuleIdentity {
        package: PackageId("game".into()),
        path: vec!["associated".into()],
    });
    let array_owner = module.definition(DefinitionKind::AssociatedType, "Bag");
    module.types.push(TypeAbi {
        name: "Bag".into(),
        kind: TypeAbiKind::Native(NativeTypeConstructor::Array),
        generic_params: vec![GenericParameterAbi {
            owner: array_owner,
            position: 0,
        }],
        bounds: vec![],
        fields: vec![],
        variants: vec![],
    });
    let source_owner = module.definition(DefinitionKind::Trait, "Source");
    let source = NominalAbiType {
        declaration: source_owner.clone(),
        arguments: vec![],
        associated_types: Default::default(),
    };
    let item = associated_type_id(&source_owner, "Item");
    let projection = AbiType::Projection {
        receiver: Box::new(AbiType::SelfType(source_owner.clone())),
        interface: Box::new(source.clone()),
        member: item.clone(),
        arguments: vec![],
    };
    module.traits.push(TraitAbi {
        name: "Source".into(),
        generic_params: vec![],
        bounds: vec![],
        supertraits: vec![],
        associated_consts: vec![],
        associated_types: vec![AssociatedTypeAbi {
            declaration: item.clone(),
            generic_params: vec![],
            parameter_bounds: vec![],
            bounds: vec![],
        }],
        methods: vec![FunctionAbi {
            name: "head".into(),
            method_policy: Default::default(),
            implementation: CallableImplementation::Required,
            generic_params: vec![],
            bounds: vec![],
            params: vec![ParameterAbi {
                name: "self".into(),
                ty: AbiType::SelfType(source_owner),
                mutable: false,
            }],
            return_type: projection,
        }],
    });
    let generic = GenericParameterAbi {
        owner: module.implementation_id(0),
        position: 0,
    };
    let mut applied = source.clone();
    applied
        .associated_types
        .insert(item.clone(), generic.as_type());
    module
        .implement_trait(
            &module.traits[0].clone(),
            applied,
            AbiType::Array(Box::new(generic.as_type()), CollectionAccess::Mutable),
            vec![generic],
            &[("head", binding_id(&module.identity, "head"))],
        )
        .unwrap();
    let hook_owner = module.definition(DefinitionKind::Trait, "Hook");
    let hook = NominalAbiType {
        declaration: hook_owner.clone(),
        arguments: vec![],
        associated_types: Default::default(),
    };
    module.traits.push(TraitAbi {
        name: "Hook".into(),
        generic_params: vec![],
        bounds: vec![],
        supertraits: vec![],
        associated_types: vec![],
        associated_consts: vec![],
        methods: vec![FunctionAbi {
            name: "check".into(),
            method_policy: Default::default(),
            implementation: CallableImplementation::Required,
            generic_params: vec![],
            bounds: vec![],
            params: vec![ParameterAbi {
                name: "self".into(),
                ty: AbiType::SelfType(hook_owner),
                mutable: false,
            }],
            return_type: AbiType::Builtin(BuiltinType::Unit),
        }],
    });
    module.implementations[0].bounds = vec![GenericBoundAbi {
        ty: module.implementations[0].generic_params[0].as_type(),
        constraints: vec![ConstraintAbi::Trait(hook.clone())],
    }];
    module.callable_requirements.insert(
        NativeModule::method_id(&module.implementation_id(0), "head"),
        vec![NativeCallableRequirement {
            receiver: module.implementations[0].generic_params[0].as_type(),
            interface: hook.clone(),
            member: NativeModule::method_id(&hook.declaration, "check"),
            arguments: vec![],
        }],
    );
    let owner = module.implementation_id(1);
    let parameter = GenericParameterAbi {
        owner: owner.clone(),
        position: 0,
    };
    let receiver = AbiType::Array(Box::new(parameter.as_type()), CollectionAccess::Mutable);
    module.implementations.push(NativeImplementation {
        generic_params: vec![parameter.clone()],
        bounds: vec![],
        trait_type: None,
        for_type: receiver.clone(),
        methods: vec![FunctionAbi {
            name: "check_first".into(),
            method_policy: Default::default(),
            implementation: CallableImplementation::Native(binding_id(
                &module.identity,
                "check_first",
            )),
            generic_params: vec![parameter.clone()],
            bounds: vec![GenericBoundAbi {
                ty: parameter.as_type(),
                constraints: vec![ConstraintAbi::Trait(hook.clone())],
            }],
            params: vec![ParameterAbi {
                name: "self".into(),
                ty: receiver.clone(),
                mutable: false,
            }],
            return_type: AbiType::Builtin(BuiltinType::Unit),
        }],
    });
    module.callable_requirements.insert(
        NativeModule::method_id(&owner, "check_first"),
        vec![NativeCallableRequirement {
            receiver: parameter.as_type(),
            interface: hook.clone(),
            member: NativeModule::method_id(&hook.declaration, "check"),
            arguments: vec![],
        }],
    );
    let owner = module.definition(DefinitionKind::Function, "echo");
    let generic = GenericParameterAbi {
        owner: owner.clone(),
        position: 0,
    };
    let projection = AbiType::Projection {
        receiver: Box::new(generic.as_type()),
        interface: Box::new(source.clone()),
        member: item.clone(),
        arguments: vec![],
    };
    module.functions.push(FunctionAbi {
        name: "echo".into(),
        method_policy: Default::default(),
        implementation: CallableImplementation::Native(binding_id(&module.identity, "echo")),
        generic_params: vec![generic.clone()],
        bounds: vec![GenericBoundAbi {
            ty: generic.as_type(),
            constraints: vec![ConstraintAbi::Trait(source.clone())],
        }],
        params: vec![
            ParameterAbi {
                name: "value".into(),
                ty: projection.clone(),
                mutable: false,
            },
            ParameterAbi {
                name: "source".into(),
                ty: generic.as_type(),
                mutable: false,
            },
        ],
        return_type: projection,
    });
    let array = AbiType::Array(
        Box::new(AbiType::Builtin(BuiltinType::I32)),
        CollectionAccess::Mutable,
    );
    let mut interface = source.clone();
    interface
        .associated_types
        .insert(item.clone(), array.clone());
    module.functions.push(FunctionAbi {
        name: "forward".into(),
        method_policy: Default::default(),
        implementation: CallableImplementation::Native(binding_id(&module.identity, "forward")),
        generic_params: vec![],
        bounds: vec![],
        params: vec![ParameterAbi {
            name: "source".into(),
            ty: AbiType::Trait(interface),
            mutable: false,
        }],
        return_type: array,
    });
    let owner = module.definition(DefinitionKind::Function, "selected_head");
    let generic = GenericParameterAbi {
        owner: owner.clone(),
        position: 0,
    };
    module.functions.push(FunctionAbi {
        name: "selected_head".into(),
        method_policy: Default::default(),
        implementation: CallableImplementation::Native(binding_id(
            &module.identity,
            "selected_head",
        )),
        generic_params: vec![generic.clone()],
        bounds: vec![GenericBoundAbi {
            ty: generic.as_type(),
            constraints: vec![ConstraintAbi::Trait(source.clone())],
        }],
        params: vec![ParameterAbi {
            name: "source".into(),
            ty: generic.as_type(),
            mutable: false,
        }],
        return_type: AbiType::Projection {
            receiver: Box::new(generic.as_type()),
            interface: Box::new(source.clone()),
            member: item.clone(),
            arguments: vec![],
        },
    });
    module.callable_requirements.insert(
        owner,
        vec![NativeCallableRequirement {
            receiver: generic.as_type(),
            interface: source.clone(),
            member: NativeModule::method_id(&source.declaration, "head"),
            arguments: vec![],
        }],
    );
    let owner = module.definition(DefinitionKind::Function, "nested_head");
    let generic = GenericParameterAbi {
        owner: owner.clone(),
        position: 0,
    };
    let output = AbiType::Projection {
        receiver: Box::new(generic.as_type()),
        interface: Box::new(source.clone()),
        member: item.clone(),
        arguments: vec![],
    };
    module.functions.push(FunctionAbi {
        name: "nested_head".into(),
        method_policy: Default::default(),
        implementation: CallableImplementation::Native(binding_id(&module.identity, "nested_head")),
        generic_params: vec![generic.clone()],
        bounds: vec![
            GenericBoundAbi {
                ty: output.clone(),
                constraints: vec![ConstraintAbi::Trait(source.clone())],
            },
            GenericBoundAbi {
                ty: generic.as_type(),
                constraints: vec![ConstraintAbi::Trait(source.clone())],
            },
        ],
        params: vec![ParameterAbi {
            name: "source".into(),
            ty: generic.as_type(),
            mutable: false,
        }],
        return_type: AbiType::Projection {
            receiver: Box::new(output.clone()),
            interface: Box::new(source.clone()),
            member: item.clone(),
            arguments: vec![],
        },
    });
    module.callable_requirements.insert(
        owner,
        [generic.as_type(), output]
            .into_iter()
            .map(|receiver| NativeCallableRequirement {
                receiver,
                interface: source.clone(),
                member: NativeModule::method_id(&source.declaration, "head"),
                arguments: vec![],
            })
            .collect(),
    );
    module
        .documentation
        .insert(item, "The value produced by this source.".into());
    let marker = module.definition(DefinitionKind::Trait, "Marker");
    for (name, associated_types) in [
        ("Marker", vec![]),
        (
            "Marked",
            vec![AssociatedTypeAbi {
                declaration: associated_type_id(
                    &module.definition(DefinitionKind::Trait, "Marked"),
                    "Item",
                ),
                generic_params: vec![],
                parameter_bounds: vec![],
                bounds: vec![ConstraintAbi::Trait(NominalAbiType {
                    declaration: marker,
                    arguments: vec![],
                    associated_types: Default::default(),
                })],
            }],
        ),
    ] {
        module.traits.push(TraitAbi {
            name: name.into(),
            generic_params: vec![],
            bounds: vec![],
            supertraits: vec![],
            methods: vec![],
            associated_types,
            associated_consts: vec![],
        });
    }
    module
}

struct Complete(Value);
impl NativeInvocationState for Complete {
    fn advance(&mut self, _: &mut NativeContext<'_>) -> Result<NativeAction, RuntimeError> {
        Ok(NativeAction::Complete(self.0.clone()))
    }
}
struct Head {
    pending: bool,
}
impl NativeInvocationState for Head {
    fn advance(&mut self, context: &mut NativeContext<'_>) -> Result<NativeAction, RuntimeError> {
        if self.pending {
            self.pending = false;
            return Ok(NativeAction::Continue);
        }
        let Some(Value::Array(id)) = context.argument(0) else {
            return Err(RuntimeError::module_validation("associated array"));
        };
        let value = context
            .heap()
            .array_get(id, 0)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::IndexOutOfBounds, "empty source"))?;
        context.retain(0, value)?;
        context
            .selected_callback(0, vec![context.retained(0).unwrap()])
            .map(NativeAction::Callback)
    }
    fn receive(
        &mut self,
        context: &mut NativeContext<'_>,
        _: Value,
    ) -> Result<NativeAction, RuntimeError> {
        Ok(NativeAction::Complete(context.retained(0).unwrap()))
    }
}
struct Forward;
impl NativeInvocationState for Forward {
    fn advance(&mut self, context: &mut NativeContext<'_>) -> Result<NativeAction, RuntimeError> {
        let value = context.argument(0).unwrap();
        let AbiType::Trait(interface) = &context.signature().params[0] else {
            return Err(RuntimeError::module_validation(
                "forward interface contract",
            ));
        };
        context
            .interface_callback(&value, interface, 0, vec![])
            .map(NativeAction::Callback)
    }
    fn receive(
        &mut self,
        context: &mut NativeContext<'_>,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        // The shared driver roots a callback's heap-backed result before receive.
        context.heap().alloc_array(vec![])?;
        context.retain(0, value)?;
        Ok(NativeAction::Complete(context.retained(0).unwrap()))
    }
}
struct CheckFirst;
impl NativeInvocationState for CheckFirst {
    fn advance(&mut self, context: &mut NativeContext<'_>) -> Result<NativeAction, RuntimeError> {
        let Some(Value::Array(id)) = context.argument(0) else {
            return Err(RuntimeError::module_validation("check receiver"));
        };
        let value = context
            .heap()
            .array_get(id, 0)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::IndexOutOfBounds, "empty check"))?;
        context
            .selected_callback(0, vec![value])
            .map(NativeAction::Callback)
    }
    fn receive(
        &mut self,
        _: &mut NativeContext<'_>,
        _: Value,
    ) -> Result<NativeAction, RuntimeError> {
        Ok(NativeAction::Complete(Value::Unit))
    }
}
struct Selected;
impl NativeInvocationState for Selected {
    fn advance(&mut self, context: &mut NativeContext<'_>) -> Result<NativeAction, RuntimeError> {
        context
            .selected_callback(0, vec![context.argument(0).unwrap()])
            .map(NativeAction::Callback)
    }
    fn receive(
        &mut self,
        context: &mut NativeContext<'_>,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        context.heap().alloc_array(vec![])?;
        context.retain(0, value)?;
        Ok(NativeAction::Complete(context.retained(0).unwrap()))
    }
}
struct Nested {
    first: bool,
}
impl NativeInvocationState for Nested {
    fn advance(&mut self, context: &mut NativeContext<'_>) -> Result<NativeAction, RuntimeError> {
        context
            .selected_callback(0, vec![context.argument(0).unwrap()])
            .map(NativeAction::Callback)
    }
    fn receive(
        &mut self,
        context: &mut NativeContext<'_>,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        context.heap().alloc_array(vec![])?;
        context.retain(0, value)?;
        if self.first {
            self.first = false;
            context
                .selected_callback(1, vec![context.retained(0).unwrap()])
                .map(NativeAction::Callback)
        } else {
            Ok(NativeAction::Complete(context.retained(0).unwrap()))
        }
    }
}
pub fn api(module: NativeModule, calls: Rc<Cell<usize>>) -> NativeApi {
    let mut handlers = vec![];
    for name in ["head", "echo"] {
        let calls = calls.clone();
        handlers.push(NativeHandler::new(
            binding_id(&module.identity, name),
            1,
            move |context| {
                calls.set(calls.get() + 1);
                let input = context
                    .argument(0)
                    .ok_or_else(|| RuntimeError::module_validation("associated input"))?;
                if name == "head" {
                    Ok(Box::new(Head { pending: true }))
                } else {
                    Ok(Box::new(Complete(input)))
                }
            },
        ));
    }
    handlers.push(NativeHandler::new(
        binding_id(&module.identity, "forward"),
        1,
        |_| Ok(Box::new(Forward)),
    ));
    handlers.push(NativeHandler::new(
        binding_id(&module.identity, "selected_head"),
        1,
        |_| Ok(Box::new(Selected)),
    ));
    handlers.push(NativeHandler::new(
        binding_id(&module.identity, "nested_head"),
        1,
        |_| Ok(Box::new(Nested { first: true })),
    ));
    handlers.push(NativeHandler::new(
        binding_id(&module.identity, "check_first"),
        0,
        |_| Ok(Box::new(CheckFirst)),
    ));
    NativeApi::new(vec![module], handlers, Default::default()).unwrap()
}
