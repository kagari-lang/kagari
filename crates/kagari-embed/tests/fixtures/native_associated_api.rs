//! Application-owned ordinary associated contracts shared by tests and generation.
use kagari_abi::{
    callable::CallableImplementation,
    native_api::NativeModule,
    native_import::binding_id,
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
            applied,
            AbiType::Array(Box::new(generic.as_type()), CollectionAccess::Mutable),
            vec![generic],
            &[("head", binding_id(&module.identity, "head"))],
        )
        .unwrap();
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
    let mut interface = source;
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
        context
            .heap()
            .array_get(id, 0)
            .map(NativeAction::Complete)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::IndexOutOfBounds, "empty source"))
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
pub fn api(module: NativeModule, calls: Rc<Cell<usize>>) -> NativeApi {
    let mut handlers = vec![];
    for name in ["head", "echo"] {
        let calls = calls.clone();
        handlers.push(NativeHandler::new(
            binding_id(&module.identity, name),
            0,
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
    NativeApi::new(vec![module], handlers).unwrap()
}
