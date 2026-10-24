//! MIR export admission is independent of bytecode and source compilation.

use crate::{
    codec::{decode_program, encode_program},
    function::MirModule,
    program::{ProgramErrorKind, records::MirProgram, verify_program},
};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext};
use kagari_contract::{
    exports::{ExportNamespace, ProgramExports, TypeExport},
    layout::StructLayout,
    types::PublicItem,
};
use kagari_types::{
    declaration::{TypeDeclaration, TypeDef, TypeDefKind},
    ty::{GenericParam, NominalTy, Ty},
};

#[test]
fn type_exports_mir_codec_retains_authority_and_rejects_layout_free_forgery() {
    let context = DefinitionContext::new().unwrap();
    let identity = ModuleIdentity::single_file("types.kgr");
    let root = context.intern_root(&identity).unwrap();
    let id = context
        .intern_child(root, DefinitionKind::Struct, "Record", 0)
        .unwrap();
    let program = MirProgram {
        update_evidence: crate::program::updates::empty(),
        root: identity.clone(),
        exports: ProgramExports {
            namespaces: [(
                identity.clone(),
                ExportNamespace {
                    types: [(
                        "Alias".into(),
                        TypeExport::Closed(Ty::Struct(NominalTy {
                            declaration: id,
                            arguments: vec![],
                            associated_types: Default::default(),
                        })),
                    )]
                    .into(),
                    ..Default::default()
                },
            )]
            .into(),
            entry: None,
        },
        modules: vec![MirModule {
            native_targets: vec![],
            interface_instances: vec![],
            external_types: vec![],
            dependencies: vec![],
            structures: vec![StructLayout {
                declaration: id,
                arguments: vec![],
                fields: vec![],
            }],
            enumerations: vec![],
            identity,
            source_name: String::new(),
            module_slots: vec![],
            abi: Default::default(),
            functions: vec![],
        }],
    };
    let mut program = program;
    program.modules[0]
        .abi
        .public_items
        .push(PublicItem::Type(TypeDef {
            name: "Record".into(),
            kind: TypeDefKind::Struct,
            generic_params: vec![],
            bounds: vec![],
            fields: vec![],
            variants: vec![],
        }));
    let verified = verify_program(program.clone(), &context, &Default::default()).unwrap();
    let bytes = encode_program(&verified, &Default::default()).unwrap();
    let decoded = decode_program(&bytes, &Default::default()).unwrap();
    assert_eq!(
        encode_program(&decoded, &Default::default()).unwrap(),
        bytes
    );
    assert_eq!(decoded.exports().namespaces.len(), 1);
    assert!(decoded.modules()[0].functions.is_empty());

    let mut no_authority = program.clone();
    no_authority.modules[0].abi.public_items.clear();
    assert!(matches!(
        verify_program(no_authority, &context, &Default::default())
            .unwrap_err()
            .kind,
        ProgramErrorKind::InvalidExports
    ));
    for export in [
        TypeExport::Declaration(id),
        program.exports.namespaces.values().next().unwrap().types["Alias"].clone(),
    ] {
        let mut missing = program.clone();
        missing.modules[0].structures.clear();
        missing
            .exports
            .namespaces
            .values_mut()
            .next()
            .unwrap()
            .types
            .insert("Alias".into(), export);
        assert!(matches!(
            verify_program(missing, &context, &Default::default())
                .unwrap_err()
                .kind,
            ProgramErrorKind::InvalidExports
        ));
    }

    let hidden = context
        .intern_child(root, DefinitionKind::Struct, "Hidden", 0)
        .unwrap();
    let mut specialized = program;
    let PublicItem::Type(record) = &mut specialized.modules[0].abi.public_items[0] else {
        panic!("nominal fixture");
    };
    record.generic_params.push(GenericParam {
        owner: id,
        position: 0,
    });
    specialized.modules[0].structures[0]
        .arguments
        .push(Ty::Struct(NominalTy {
            declaration: hidden,
            arguments: vec![],
            associated_types: Default::default(),
        }));
    specialized.modules[0].structures.push(StructLayout {
        declaration: hidden,
        arguments: vec![],
        fields: vec![],
    });
    specialized.modules[0]
        .abi
        .private_types
        .push(TypeDeclaration {
            declaration: hidden,
            definition: TypeDef {
                name: "Hidden".into(),
                kind: TypeDefKind::Struct,
                generic_params: vec![],
                bounds: vec![],
                fields: vec![],
                variants: vec![],
            },
        });
    specialized
        .exports
        .namespaces
        .values_mut()
        .next()
        .unwrap()
        .types = [("Record".into(), TypeExport::Declaration(id))].into();
    let verified = verify_program(specialized, &context, &Default::default()).unwrap();
    let bytes = encode_program(&verified, &Default::default()).unwrap();
    let decoded = decode_program(&bytes, &Default::default()).unwrap();
    assert_eq!(decoded.modules()[0].abi.private_types.len(), 1);
    assert_eq!(decoded.modules()[0].abi.public_items.len(), 1);
    assert_eq!(
        encode_program(&decoded, &Default::default()).unwrap(),
        bytes
    );
}
