use super::source_free_vm;
use kagari_runtime::{
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        declarations::FunctionDecl,
        payload::Opaque,
        schema::{TypeRegistration, ValueField},
        typed::NativeContext,
        types::Type,
    },
    value::view::ValueView,
};

use crate::declarations::StandardDeclarations;

kagari_runtime::native_data! { struct Shared { len: u64 } }

struct Packet {
    hp: i32,
    shared: Opaque<Shared>,
}

impl KagariType for Packet {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog.rust_type::<Self>()
    }

    fn register_type(cx: &mut TypeRegistration<'_>) -> NativeResult<Type> {
        cx.structure::<Self>("Packet", |cx| {
            Ok(vec![
                ValueField::new("hp", cx.dependency::<i32>()?),
                ValueField::new("shared", cx.dependency::<Opaque<Shared>>()?),
            ])
        })
    }
}

impl IntoKagari for Packet {
    fn into_kagari<'call>(
        self,
        cx: &mut ConversionContext<'call>,
        expected: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
        let mut fields = cx.write_struct(expected)?;
        fields.field("hp", self.hp)?;
        fields.field("shared", self.shared)?;
        fields.finish()
    }
}

impl FromKagari for Packet {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ValueView<'_>,
    ) -> NativeResult<Self> {
        let mut fields = cx.read_struct(expected, value)?;
        Ok(Self {
            hp: fields.field("hp")?,
            shared: fields.field("shared")?,
        })
    }
}

#[test]
fn nominal_dtos_use_typed_callbacks_while_explicit_handles_keep_shared_identity() {
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let mut module = ModuleBuilder::new("example::dto", &catalog).unwrap();
    let packet = module.register_type::<Packet>().unwrap();
    let shared = Opaque::<Shared>::kagari_type(module.declarations()).unwrap();
    let make = module
        .define_function(FunctionDecl::new("shared").returns(shared.clone()))
        .unwrap();
    module
        .bind(
            make,
            |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<Opaque<Shared>> {
                cx.create_opaque(Shared { len: 1 })
            },
        )
        .unwrap();
    let len = module
        .define_function(
            FunctionDecl::new("len")
                .parameter("shared", shared)
                .returns(Type::u64()),
        )
        .unwrap();
    module
        .bind(
            len,
            |cx: &mut NativeContext<'_>, (shared,): (Opaque<Shared>,)| -> NativeResult<u64> {
                shared.read(cx, |shared| Ok(shared.len))
            },
        )
        .unwrap();
    let update = module
        .define_function(
            FunctionDecl::new("update")
                .parameter("packet", packet.clone())
                .returns(packet),
        )
        .unwrap();
    module
        .bind(
            update,
            |cx: &mut NativeContext<'_>, (mut packet,): (Packet,)| -> NativeResult<Packet> {
                packet.hp += 1;
                packet.shared.edit(cx, |shared| {
                    shared.len += 1;
                    Ok(())
                })?;
                cx.collect_garbage()?;
                Ok(packet)
            },
        )
        .unwrap();
    let (mut vm, owner) = source_free_vm(
        include_str!("../../../../../tests/fixtures/native/conversion-packet-identity.kgr"),
        &module.finish().unwrap(),
    );
    let _owner_program = owner;
    let _owner = vm.select_program(&_owner_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    assert_eq!(
        vm.execute_typed::<_, (i32, i32, usize)>(&_owner_program, "main", (), &Default::default())
            .unwrap(),
        (7, 8, 2)
    );
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}
