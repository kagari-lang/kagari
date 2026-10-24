use crate::state::{
    data::*,
    error::{SnapshotErrorKind, SnapshotResult, SnapshotStage},
    limits::{BYTE_CHUNK, Budget, SnapshotLimits, SnapshotOptions, validate_schema},
    wire::{Body, MAGIC, decode::text_chunk, options},
};
use bincode::Options;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

fn version() -> SchemaVersion {
    SchemaVersion::new("player", 1).unwrap()
}

fn header() -> SnapshotHeader {
    SnapshotHeader {
        schema: version(),
        source_deployment: "old-build".into(),
        state_owner: "player-7".into(),
        state_revision: 42,
    }
}

fn schema(root: StateType) -> StateSchema {
    StateSchema {
        version: version(),
        root,
        definitions: vec![],
    }
}

fn owned_snapshot(
    header: SnapshotHeader,
    schema: StateSchema,
    payload: StateValue,
    options: &SnapshotOptions,
) -> SnapshotResult<StateSnapshot> {
    StateSnapshot::new(
        header,
        schema,
        payload,
        &mut Budget::new(options, SnapshotStage::Validate),
    )
}

fn snapshot(ty: StateType, value: StateValue) -> StateSnapshot {
    owned_snapshot(header(), schema(ty), value, &SnapshotOptions::default()).unwrap()
}

fn raw(header: &SnapshotHeader, schema: &StateSchema, payload: &StateValue) -> Vec<u8> {
    let body = options(u64::MAX)
        .serialize(&Body {
            header,
            schema,
            payload,
        })
        .unwrap();
    let mut bytes = MAGIC.to_vec();
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&(body.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&body);
    bytes
}

fn decode_kind(bytes: &[u8]) -> SnapshotErrorKind {
    StateSnapshot::from_bytes(bytes, &SnapshotOptions::default())
        .unwrap_err()
        .kind
}

fn key(name: &str) -> NominalKey {
    NominalKey {
        package: "game".into(),
        module: vec!["dto".into()],
        declaration: vec![name.into()],
        arguments: vec![],
    }
}

fn record() -> (StateSchema, StateValue) {
    let key = key("Player");
    let mut schema = schema(StateType::Nominal(key.clone()));
    schema.definitions.push(StateDefinition {
        key: key.clone(),
        shape: StateShape::Struct(vec![
            StateField {
                name: "hp".into(),
                ty: StateType::Scalar(StateScalar::I32),
            },
            StateField {
                name: "name".into(),
                ty: StateType::Scalar(StateScalar::String),
            },
        ]),
    });
    (
        schema,
        StateValue::Record {
            key,
            fields: vec![
                ("hp".into(), StateValue::I32(10)),
                ("name".into(), StateValue::String("Ada".into())),
            ],
        },
    )
}

#[test]
fn scalar_tags_widths_and_float_bits_are_exact() {
    let cases = [
        (StateScalar::Unit, StateValue::Unit, 0, vec![]),
        (StateScalar::Bool, StateValue::Bool(true), 1, vec![1]),
        (
            StateScalar::Char,
            StateValue::Char('🦀'),
            2,
            u32::from('🦀').to_le_bytes().to_vec(),
        ),
        (
            StateScalar::I8,
            StateValue::I8(i8::MIN),
            3,
            i8::MIN.to_le_bytes().to_vec(),
        ),
        (
            StateScalar::I16,
            StateValue::I16(i16::MIN),
            4,
            i16::MIN.to_le_bytes().to_vec(),
        ),
        (
            StateScalar::I32,
            StateValue::I32(i32::MIN),
            5,
            i32::MIN.to_le_bytes().to_vec(),
        ),
        (
            StateScalar::I64,
            StateValue::I64(i64::MIN),
            6,
            i64::MIN.to_le_bytes().to_vec(),
        ),
        (
            StateScalar::U8,
            StateValue::U8(u8::MAX),
            7,
            u8::MAX.to_le_bytes().to_vec(),
        ),
        (
            StateScalar::U16,
            StateValue::U16(u16::MAX),
            8,
            u16::MAX.to_le_bytes().to_vec(),
        ),
        (
            StateScalar::U32,
            StateValue::U32(u32::MAX),
            9,
            u32::MAX.to_le_bytes().to_vec(),
        ),
        (
            StateScalar::U64,
            StateValue::U64(u64::MAX),
            10,
            u64::MAX.to_le_bytes().to_vec(),
        ),
        (
            StateScalar::F32,
            StateValue::F32Bits(0xffc01234),
            11,
            0xffc01234_u32.to_le_bytes().to_vec(),
        ),
        (
            StateScalar::F64,
            StateValue::F64Bits(0xfff8123456789abc),
            12,
            0xfff8123456789abc_u64.to_le_bytes().to_vec(),
        ),
    ];
    for (ty, value, tag, encoded) in cases {
        let snapshot = snapshot(StateType::Scalar(ty), value);
        let bytes = snapshot.to_bytes(&SnapshotOptions::default()).unwrap();
        let mut suffix = (tag as u32).to_le_bytes().to_vec();
        suffix.extend_from_slice(&encoded);
        assert!(bytes.ends_with(&suffix));
        assert_eq!(
            StateSnapshot::from_bytes(&bytes, &SnapshotOptions::default()).unwrap(),
            snapshot
        );
    }
    for bits in [
        0,
        1 << 31,
        f32::INFINITY.to_bits(),
        f32::NEG_INFINITY.to_bits(),
        0x7f800001,
    ] {
        let snapshot = snapshot(
            StateType::Scalar(StateScalar::F32),
            StateValue::F32Bits(bits),
        );
        assert_eq!(
            StateSnapshot::from_bytes(
                &snapshot.to_bytes(&SnapshotOptions::default()).unwrap(),
                &SnapshotOptions::default()
            )
            .unwrap(),
            snapshot
        );
    }
    for bits in [
        0,
        1 << 63,
        f64::INFINITY.to_bits(),
        f64::NEG_INFINITY.to_bits(),
        0x7ff0000000000001,
    ] {
        let snapshot = snapshot(
            StateType::Scalar(StateScalar::F64),
            StateValue::F64Bits(bits),
        );
        assert_eq!(
            StateSnapshot::from_bytes(
                &snapshot.to_bytes(&SnapshotOptions::default()).unwrap(),
                &SnapshotOptions::default()
            )
            .unwrap(),
            snapshot
        );
    }
}

#[test]
fn framing_trailing_tags_unicode_and_numeric_mismatch_are_rejected() {
    let snapshot = snapshot(StateType::Scalar(StateScalar::Char), StateValue::Char('a'));
    let valid = snapshot.to_bytes(&SnapshotOptions::default()).unwrap();
    for offset in [0, 8, 10, 12] {
        let mut bytes = valid.clone();
        bytes[offset] ^= 0x40;
        assert_eq!(decode_kind(&bytes), SnapshotErrorKind::InvalidHeader);
    }
    for length in [0, 7, 19, valid.len() - 1] {
        assert_eq!(
            decode_kind(&valid[..length]),
            SnapshotErrorKind::InvalidHeader
        );
    }
    let mut trailing = valid.clone();
    trailing.push(0);
    assert_eq!(decode_kind(&trailing), SnapshotErrorKind::InvalidHeader);
    let length = (trailing.len() - 20) as u64;
    trailing[12..20].copy_from_slice(&length.to_le_bytes());
    assert_eq!(decode_kind(&trailing), SnapshotErrorKind::InvalidWire);
    for code in [0xd800_u32, 0xdfff, 0x110000, u32::MAX] {
        let mut bytes = valid.clone();
        let end = bytes.len();
        bytes[end - 4..].copy_from_slice(&code.to_le_bytes());
        assert_eq!(decode_kind(&bytes), SnapshotErrorKind::InvalidWire);
    }
    let mut bytes = valid.clone();
    let end = bytes.len();
    bytes[end - 8..end - 4].copy_from_slice(&18_u32.to_le_bytes());
    assert_eq!(decode_kind(&bytes), SnapshotErrorKind::InvalidWire);
    let bytes = raw(
        &header(),
        &schema(StateType::Scalar(StateScalar::U64)),
        &StateValue::I64(1),
    );
    assert_eq!(decode_kind(&bytes), SnapshotErrorKind::TypeMismatch);
    let mut invalid_header = header();
    invalid_header.schema.version = 0;
    assert_eq!(
        decode_kind(&raw(&invalid_header, snapshot.schema(), snapshot.payload())),
        SnapshotErrorKind::InvalidSchema
    );
    assert!(SchemaVersion::new("x", 0).is_err());
    let root_offset = 20
        + options(u64::MAX)
            .serialize(&(header(), version()))
            .unwrap()
            .len();
    for offset in [root_offset, root_offset + 4] {
        let mut bytes = valid.clone();
        bytes[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode_kind(&bytes), SnapshotErrorKind::InvalidSchema);
    }
    for (ty, value) in [
        (StateScalar::Bool, StateValue::Bool(true)),
        (StateScalar::String, StateValue::String("a".into())),
    ] {
        let mut bytes = raw(&header(), &schema(StateType::Scalar(ty)), &value);
        *bytes.last_mut().unwrap() = 0xff;
        assert_eq!(decode_kind(&bytes), SnapshotErrorKind::InvalidWire);
    }
    let string_schema = schema(StateType::Scalar(StateScalar::String));
    for scalar in ["é", "€", "🦀"] {
        for split in 1..scalar.len() {
            let text = format!(
                "{}{scalar}{}",
                "a".repeat(BYTE_CHUNK - split),
                "b".repeat(BYTE_CHUNK)
            );
            let payload = StateValue::String(text);
            let bytes = raw(&header(), &string_schema, &payload);
            let decoded = StateSnapshot::from_bytes(&bytes, &SnapshotOptions::default()).unwrap();
            assert_eq!(decoded.payload(), &payload);
            assert_eq!(
                decoded.to_bytes(&SnapshotOptions::default()).unwrap(),
                bytes
            );
        }
    }
    for suffix in [
        vec![0xc0, 0xaf],
        vec![0xed, 0xa0, 0x80],
        vec![0xf4, 0x90, 0x80, 0x80],
        vec![0xe2, 0x82],
        vec![0x80; BYTE_CHUNK + 4],
    ] {
        let mut invalid = vec![b'a'; BYTE_CHUNK - 1];
        invalid.extend_from_slice(&suffix);
        let payload = StateValue::String("a".repeat(invalid.len()));
        let mut bytes = raw(&header(), &string_schema, &payload);
        let start = bytes.len() - invalid.len();
        bytes[start..].copy_from_slice(&invalid);
        assert_eq!(decode_kind(&bytes), SnapshotErrorKind::InvalidWire);
    }
}

#[test]
fn record_members_and_schema_names_are_canonical_and_complete() {
    let (schema, payload) = record();
    let valid = raw(&header(), &schema, &payload);
    assert!(StateSnapshot::from_bytes(&valid, &SnapshotOptions::default()).is_ok());
    for (fields, kind) in [
        (
            vec![("hp", StateValue::I32(1)), ("hp", StateValue::I32(2))],
            SnapshotErrorKind::Duplicate,
        ),
        (
            vec![
                ("name", StateValue::String("a".into())),
                ("hp", StateValue::I32(1)),
            ],
            SnapshotErrorKind::NonCanonical,
        ),
        (
            vec![("hp", StateValue::I32(1))],
            SnapshotErrorKind::MissingMember,
        ),
        (
            vec![("extra", StateValue::Unit)],
            SnapshotErrorKind::UnknownMember,
        ),
        (
            vec![
                ("hp", StateValue::U32(1)),
                ("name", StateValue::String("a".into())),
            ],
            SnapshotErrorKind::TypeMismatch,
        ),
    ] {
        let value = StateValue::Record {
            key: key("Player"),
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        };
        assert_eq!(decode_kind(&raw(&header(), &schema, &value)), kind);
    }
    let mut duplicate = schema.clone();
    duplicate.definitions.push(duplicate.definitions[0].clone());
    assert_eq!(
        decode_kind(&raw(&header(), &duplicate, &payload)),
        SnapshotErrorKind::Duplicate
    );
    let mut duplicate = schema.clone();
    let StateShape::Struct(fields) = &mut duplicate.definitions[0].shape else {
        unreachable!()
    };
    fields[1].name = fields[0].name.clone();
    assert_eq!(
        decode_kind(&raw(&header(), &duplicate, &payload)),
        SnapshotErrorKind::Duplicate
    );
}

#[test]
fn complete_schema_checks_empty_branches_and_supports_nominal_recursion() {
    let nominal = key("List");
    let mut schema = schema(StateType::Nominal(nominal.clone()));
    schema.definitions.push(StateDefinition {
        key: nominal.clone(),
        shape: StateShape::Enum(vec![
            StateVariant {
                name: "Cons".into(),
                fields: vec![
                    StateType::Scalar(StateScalar::I32),
                    StateType::Nominal(nominal.clone()),
                ],
            },
            StateVariant {
                name: "Nil".into(),
                fields: vec![],
            },
        ]),
    });
    let payload = StateValue::Enum {
        key: nominal.clone(),
        variant: "Nil".into(),
        fields: vec![],
    };
    let snapshot = owned_snapshot(
        header(),
        schema.clone(),
        payload.clone(),
        &SnapshotOptions::default(),
    )
    .unwrap();
    assert_eq!(
        StateSnapshot::from_bytes(
            &snapshot.to_bytes(&SnapshotOptions::default()).unwrap(),
            &SnapshotOptions::default()
        )
        .unwrap(),
        snapshot
    );
    let StateShape::Enum(variants) = &mut schema.definitions[0].shape else {
        unreachable!()
    };
    variants[0].fields[1] = StateType::Nominal(key("Missing"));
    assert_eq!(
        decode_kind(&raw(&header(), &schema, &payload)),
        SnapshotErrorKind::InvalidSchema
    );
    let empty_schema = self::schema(StateType::Sequence(Box::new(StateType::Nominal(key(
        "Missing",
    )))));
    assert_eq!(
        decode_kind(&raw(
            &header(),
            &empty_schema,
            &StateValue::Sequence(vec![])
        )),
        SnapshotErrorKind::InvalidSchema
    );
    let mut unused = snapshot.schema().clone();
    unused.root = StateType::Scalar(StateScalar::Unit);
    assert_eq!(
        decode_kind(&raw(&header(), &unused, &StateValue::Unit)),
        SnapshotErrorKind::InvalidSchema
    );
    let mut generic = key("Closed");
    generic
        .arguments
        .push(StateType::Nominal(key("MissingArgument")));
    let mut closed = self::schema(StateType::Nominal(generic.clone()));
    closed.definitions.push(StateDefinition {
        key: generic.clone(),
        shape: StateShape::Struct(vec![]),
    });
    assert_eq!(
        decode_kind(&raw(
            &header(),
            &closed,
            &StateValue::Record {
                key: generic,
                fields: vec![]
            }
        )),
        SnapshotErrorKind::InvalidSchema
    );
}

#[test]
fn every_admission_limit_applies_to_owned_and_wire_data() {
    let sequence = snapshot(
        StateType::Sequence(Box::new(StateType::Scalar(StateScalar::Unit))),
        StateValue::Sequence(vec![StateValue::Unit; 3]),
    );
    let bytes = sequence.to_bytes(&SnapshotOptions::default()).unwrap();
    let limits = SnapshotLimits {
        max_nodes: 3,
        ..SnapshotLimits::default()
    };
    let mut options = SnapshotOptions {
        limits,
        ..SnapshotOptions::default()
    };
    assert!(sequence.to_bytes(&options).is_err());
    assert!(StateSnapshot::from_bytes(&bytes, &options).is_err());
    options.limits.max_nodes = 4;
    assert!(sequence.to_bytes(&options).is_ok());
    assert!(StateSnapshot::from_bytes(&bytes, &options).is_ok());
    options.limits.max_sequence_len = 2;
    assert!(StateSnapshot::from_bytes(&bytes, &options).is_err());
    options = SnapshotOptions::default();
    options.limits.max_bytes = bytes.len() - 1;
    assert!(sequence.to_bytes(&options).is_err());
    assert!(StateSnapshot::from_bytes(&bytes, &options).is_err());
    options.limits.max_bytes += 1;
    assert_eq!(sequence.to_bytes(&options).unwrap(), bytes);
    options.limits.max_work = 1;
    assert!(sequence.to_bytes(&options).is_err());
    assert!(StateSnapshot::from_bytes(&bytes, &options).is_err());
    options = SnapshotOptions::default();
    options.cancellation.cancel();
    assert_eq!(
        sequence.to_bytes(&options).unwrap_err().kind,
        SnapshotErrorKind::Cancelled
    );
    assert_eq!(
        StateSnapshot::from_bytes(&bytes, &options)
            .unwrap_err()
            .kind,
        SnapshotErrorKind::Cancelled
    );

    let text = snapshot(
        StateType::Scalar(StateScalar::String),
        StateValue::String("éé".into()),
    );
    let text_bytes = text.to_bytes(&SnapshotOptions::default()).unwrap();
    for (limit, value) in [
        ("string", 3),
        ("identifier", 5),
        ("total", 20),
        ("schema", 0),
    ] {
        let mut options = SnapshotOptions::default();
        match limit {
            "string" => options.limits.max_string_bytes = value,
            "identifier" => options.limits.max_identifier_bytes = value,
            "total" => options.limits.max_total_string_bytes = value,
            _ => options.limits.max_schema_nodes = value,
        }
        assert!(text.to_bytes(&options).is_err(), "{limit}");
        assert!(
            StateSnapshot::from_bytes(&text_bytes, &options).is_err(),
            "{limit}"
        );
    }
    let (schema, payload) = record();
    for definitions in [true, false] {
        let mut options = SnapshotOptions::default();
        if definitions {
            options.limits.max_definitions = 0;
        } else {
            options.limits.max_fields = 1;
        }
        assert!(owned_snapshot(header(), schema.clone(), payload.clone(), &options).is_err());
        assert!(StateSnapshot::from_bytes(&raw(&header(), &schema, &payload), &options).is_err());
    }

    // Exercise the actual chunk scanner, cancelling after one successful chunk
    // rather than only testing cancellation at the envelope entrypoint.
    let text = format!("{}🦀{}", "a".repeat(BYTE_CHUNK - 1), "b".repeat(BYTE_CHUNK));
    let options = SnapshotOptions::default();
    let mut budget = Budget::new(&options, SnapshotStage::Decode);
    let first = text_chunk(text.as_bytes(), &mut budget).unwrap();
    assert_eq!(first.len(), BYTE_CHUNK - 1);
    options.cancellation.cancel();
    assert_eq!(
        text_chunk(&text.as_bytes()[first.len()..], &mut budget)
            .unwrap_err()
            .kind,
        SnapshotErrorKind::Cancelled
    );

    let mut bytes = vec![b'a'; BYTE_CHUNK];
    bytes.extend_from_slice(&vec![0xff; BYTE_CHUNK]);
    let mut options = SnapshotOptions::default();
    options.limits.max_work = (BYTE_CHUNK + 16) as u64;
    let mut budget = Budget::new(&options, SnapshotStage::Decode);
    let first = text_chunk(&bytes, &mut budget).unwrap();
    assert_eq!(first.len(), BYTE_CHUNK);
    assert_eq!(
        text_chunk(&bytes[first.len()..], &mut budget)
            .unwrap_err()
            .kind,
        SnapshotErrorKind::LimitExceeded { limit: "work" }
    );
}

#[test]
fn huge_counts_and_lengths_are_rejected_before_allocation() {
    let empty = snapshot(
        StateType::Sequence(Box::new(StateType::Scalar(StateScalar::Unit))),
        StateValue::Sequence(vec![]),
    );
    let mut bytes = empty.to_bytes(&SnapshotOptions::default()).unwrap();
    let end = bytes.len();
    bytes[end - 8..].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(matches!(
        decode_kind(&bytes),
        SnapshotErrorKind::LimitExceeded { .. }
    ));
    // String lengths are borrowed/admitted without a Vec allocation by the reader.
    let text = snapshot(
        StateType::Scalar(StateScalar::String),
        StateValue::String(String::new()),
    );
    let mut bytes = text.to_bytes(&SnapshotOptions::default()).unwrap();
    let end = bytes.len();
    bytes[end - 8..].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(decode_kind(&bytes), SnapshotErrorKind::InvalidWire);

    let mut header = header();
    header.schema.id = "a".repeat(2 * BYTE_CHUNK);
    let mut schema = schema(StateType::Scalar(StateScalar::Unit));
    schema.version = header.schema.clone();
    let mut bytes = raw(&header, &schema, &StateValue::Unit);
    let mut options = SnapshotOptions::default();
    options.limits.max_identifier_bytes = 64;
    assert_eq!(
        StateSnapshot::from_bytes(&bytes, &options)
            .unwrap_err()
            .kind,
        SnapshotErrorKind::LimitExceeded {
            limit: "identifier_bytes"
        }
    );
    // A malformed suffix must not be inspected before length/work admission.
    bytes[20 + 8 + header.schema.id.len() - 1] = 0xff;
    assert_eq!(
        StateSnapshot::from_bytes(&bytes, &options)
            .unwrap_err()
            .kind,
        SnapshotErrorKind::LimitExceeded {
            limit: "identifier_bytes"
        }
    );
    options.limits.max_identifier_bytes = header.schema.id.len();
    options.limits.max_total_string_bytes = 64;
    assert_eq!(
        StateSnapshot::from_bytes(&bytes, &options)
            .unwrap_err()
            .kind,
        SnapshotErrorKind::LimitExceeded {
            limit: "total_string_bytes"
        }
    );
    options.limits.max_total_string_bytes = SnapshotLimits::default().max_total_string_bytes;
    options.limits.max_work = 64;
    assert_eq!(
        StateSnapshot::from_bytes(&bytes, &options)
            .unwrap_err()
            .kind,
        SnapshotErrorKind::LimitExceeded { limit: "work" }
    );
}

#[test]
fn recursive_traversal_obeys_the_hard_depth_ceiling() {
    let mut ty = StateType::Scalar(StateScalar::Unit);
    let mut value = StateValue::Unit;
    for _ in 1..64 {
        ty = StateType::Tuple(vec![ty]);
        value = StateValue::Tuple(vec![value]);
    }
    let snapshot = snapshot(ty.clone(), value.clone());
    assert!(snapshot.to_bytes(&SnapshotOptions::default()).is_ok());
    let mut options = SnapshotOptions::default();
    options.limits.max_depth = usize::MAX;
    let too_deep = schema(StateType::Tuple(vec![ty]));
    let payload = StateValue::Tuple(vec![value]);
    assert!(owned_snapshot(header(), too_deep.clone(), payload.clone(), &options).is_err());
    assert!(StateSnapshot::from_bytes(&raw(&header(), &too_deep, &payload), &options).is_err());
    options.limits.max_depth = 1;
    assert!(snapshot.to_bytes(&options).is_err());

    let leaf_key = key("Leaf");
    let mut leaf_schema = schema(StateType::Nominal(leaf_key.clone()));
    leaf_schema.definitions.push(StateDefinition {
        key: leaf_key.clone(),
        shape: StateShape::Struct(vec![]),
    });
    let leaf = StateValue::Record {
        key: leaf_key,
        fields: vec![],
    };
    let mut bytes = raw(&header(), &leaf_schema, &leaf);
    assert!(StateSnapshot::from_bytes(&bytes, &options).is_ok());
    // Claim one child, but provide no name or value. Depth must reject before
    // reserving the child vector or attempting to read that missing first name.
    let end = bytes.len();
    bytes[end - 8..].copy_from_slice(&1_u64.to_le_bytes());
    assert_eq!(
        StateSnapshot::from_bytes(&bytes, &options)
            .unwrap_err()
            .kind,
        SnapshotErrorKind::LimitExceeded { limit: "depth" }
    );
}

#[test]
fn migration_checks_versions_complete_targets_output_and_preserves_provenance() {
    let old = snapshot(StateType::Scalar(StateScalar::I32), StateValue::I32(7));
    let mut target = schema(StateType::Scalar(StateScalar::I64));
    target.version.version = 2;
    let new = old
        .clone()
        .migrate(
            &version(),
            &target,
            &SnapshotOptions::default(),
            |cx, value| {
                assert_eq!(cx.source(), old.header());
                assert_eq!(cx.target(), &target);
                cx.charge_work(1)?;
                let StateValue::I32(value) = value else {
                    panic!("expected i32")
                };
                Ok(StateValue::I64(i64::from(value)))
            },
        )
        .unwrap();
    assert_eq!(new.payload(), &StateValue::I64(7));
    assert_eq!(new.header().schema, target.version);
    assert_eq!(new.header().state_owner, old.header().state_owner);
    assert_eq!(new.header().state_revision, 42);
    assert_eq!(
        new.header().source_deployment,
        old.header().source_deployment
    );
    let called = AtomicBool::new(false);
    let invalid = StateSnapshot::from_validated(header(), old.schema().clone(), StateValue::U32(7));
    assert_eq!(
        invalid
            .migrate(
                &version(),
                &target,
                &SnapshotOptions::default(),
                |_, value| {
                    called.store(true, Ordering::Relaxed);
                    Ok(value)
                }
            )
            .unwrap_err()
            .kind,
        SnapshotErrorKind::TypeMismatch
    );
    assert!(!called.load(Ordering::Relaxed));
    let wrong = SchemaVersion::new("other", 1).unwrap();
    assert!(
        old.clone()
            .migrate(&wrong, &target, &SnapshotOptions::default(), |_, value| {
                called.store(true, Ordering::Relaxed);
                Ok(value)
            })
            .is_err()
    );
    assert!(!called.load(Ordering::Relaxed));
    let mut invalid = target.clone();
    invalid.root = StateType::Sequence(Box::new(StateType::Nominal(key("Missing"))));
    assert!(
        old.clone()
            .migrate(
                &version(),
                &invalid,
                &SnapshotOptions::default(),
                |_, value| {
                    called.store(true, Ordering::Relaxed);
                    Ok(value)
                }
            )
            .is_err()
    );
    assert!(!called.load(Ordering::Relaxed));
    let mut unchanged = target.clone();
    unchanged.version = version();
    assert!(
        old.clone()
            .migrate(
                &version(),
                &unchanged,
                &SnapshotOptions::default(),
                |_, value| Ok(value)
            )
            .is_err()
    );
    assert_eq!(
        old.clone()
            .migrate(
                &version(),
                &target,
                &SnapshotOptions::default(),
                |_, value| Ok(value)
            )
            .unwrap_err()
            .kind,
        SnapshotErrorKind::TypeMismatch
    );
    assert_eq!(
        old.clone()
            .migrate(
                &version(),
                old.schema(),
                &SnapshotOptions::default(),
                |_, value| Ok(value)
            )
            .unwrap(),
        old
    );
}

#[test]
fn migration_bounds_cancellation_errors_and_unwind_are_owned_only() {
    let old = snapshot(
        StateType::Scalar(StateScalar::String),
        StateValue::String("old".into()),
    );
    let mut options = SnapshotOptions::default();
    options.limits.max_string_bytes = 3;
    assert!(
        old.clone()
            .migrate(&version(), old.schema(), &options, |_, _| Ok(
                StateValue::String("large".into())
            ))
            .is_err()
    );
    assert_eq!(
        old.clone()
            .migrate(&version(), old.schema(), &options, |cx, value| {
                cx.cancellation().cancel();
                Ok(value)
            })
            .unwrap_err()
            .kind,
        SnapshotErrorKind::Cancelled
    );
    let dropped = Arc::new(AtomicBool::new(false));
    struct Guard(Arc<AtomicBool>);

    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }
    let guard = Guard(dropped.clone());
    let result = catch_unwind(AssertUnwindSafe(|| {
        old.clone().migrate(
            &version(),
            old.schema(),
            &SnapshotOptions::default(),
            |_, _| {
                let _guard = guard;
                panic!("trusted hook panic");
            },
        )
    }));
    assert!(result.is_err());
    assert!(dropped.load(Ordering::Relaxed));
    assert_eq!(old.payload(), &StateValue::String("old".into()));
}

#[test]
fn owned_snapshots_are_send_sync_and_transfer_across_threads() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<StateSnapshot>();
    send_sync::<StateSchema>();
    send_sync::<StateValue>();
    send_sync::<SnapshotHeader>();
    send_sync::<Vec<u8>>();
    let (schema, value) = record();
    validate_schema(
        &schema,
        &mut Budget::new(&SnapshotOptions::default(), SnapshotStage::Validate),
    )
    .unwrap();
    let snapshot = owned_snapshot(header(), schema, value, &SnapshotOptions::default()).unwrap();
    let expected = snapshot.clone();
    let actual = std::thread::spawn(move || {
        let bytes = snapshot.to_bytes(&SnapshotOptions::default()).unwrap();
        StateSnapshot::from_bytes(&bytes, &SnapshotOptions::default()).unwrap()
    })
    .join()
    .unwrap();
    assert_eq!(actual, expected);
}
