use super::provider::{self, Cursor, Observations, OrderedMap};
use kagari_runtime::native::{
    binding::NativeResult,
    builder::ModuleBuilder,
    declarations::{FunctionDecl, MethodDecl},
    module::NativeModule,
    payload::Opaque,
    typed::NativeContext,
    types::{Type, TypeRef},
};
use kagari_stdlib::declarations::StandardDeclarations;
use std::sync::Arc;

pub fn module(observations: Arc<Observations>) -> NativeResult<(NativeModule, TypeRef)> {
    let language = StandardDeclarations::default();
    let mut module = ModuleBuilder::new("external::ordered", &language.catalog()?)?;
    let mut declaration = module.define_opaque::<OrderedMap>("OrderedMap")?;
    declaration.type_parameter("T")?;
    let map = declaration.finish()?;
    let mut declaration = module.define_opaque::<Cursor>("Cursor")?;
    declaration.type_parameter("T")?;
    let cursor = declaration.finish()?;
    module.implement(cursor.clone(), |group| {
        let item = Type::tuple([Type::i32(), group.parameter("T")?.ty()])?;
        group.trait_impl(
            language.iterator().apply([])?.associated("Item", item)?,
            |methods| methods.bind("next", provider::next),
        )
    })?;
    module.implement(map.clone(), |group| {
        let item = group.parameter("T")?.ty();
        let receiver = group.receiver();
        group.inherent_impl(|methods| {
            let new = methods.define_method(MethodDecl::static_method("new").returns(receiver))?;
            methods.bind(
                new,
                move |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<Opaque<OrderedMap>> {
                    let ty = cx.result_native_type::<OrderedMap>()?;
                    let payload = OrderedMap::empty(cx, &ty, observations.clone())?;
                    ty.create(cx, payload, &[])
                },
            )?;
            let callback = methods.define_method(
                MethodDecl::instance("with_scratch")
                    .parameter("callback", Type::function([], Type::unit())?),
            )?;
            methods.bind(callback, provider::callback)
        })?;
        group.trait_impl(
            language
                .iterable()
                .apply([])?
                .associated("Item", Type::tuple([Type::i32(), item.clone()])?)?
                .associated("Iter", cursor.apply([item.clone()])?)?,
            |methods| methods.bind("iter", provider::iter),
        )?;
        group.trait_impl(
            language.map().apply([Type::i32(), item.clone()])?,
            |methods| {
                methods.bind("len", provider::len)?;
                methods.bind("is_empty", provider::is_empty)?;
                methods.bind("contains_key", provider::contains)?;
                methods.bind("get", provider::get)
            },
        )?;
        group.trait_impl(
            language.mutable_map().apply([Type::i32(), item])?,
            |methods| {
                methods.bind("insert", provider::insert)?;
                methods.bind("remove", provider::remove)?;
                methods.bind("clear", provider::clear)
            },
        )
    })?;
    let collect = module.define_function(FunctionDecl::new("collect"))?;
    module.bind(
        collect,
        |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
            cx.collect_garbage()?;
            Ok(())
        },
    )?;
    Ok((module.finish()?, map))
}
