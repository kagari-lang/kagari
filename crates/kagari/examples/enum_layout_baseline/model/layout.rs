//! Packed byte-record layout model; all scalar access uses byte codecs, never casts.
use std::{collections::HashSet, mem::size_of};

const INLINE_BYTES: usize = 24;

#[derive(Clone, Copy, Debug)]
struct Niche {
    offset: usize,
    width: usize,
    first: u64,
    last: u64,
}

#[derive(Clone, Copy, Debug)]
struct Shape {
    bytes: usize,
    inhabited: bool,
    niche: Option<Niche>,
}

#[derive(Debug, PartialEq, Eq)]
enum Encoding {
    Empty,
    Single,
    Direct,
    Heap,
    Niche { offset: usize, first: u64 },
}

#[derive(Debug)]
struct Planned {
    shape: Shape,
    encoding: Encoding,
}

const UNIT: Shape = Shape {
    bytes: 0,
    inhabited: true,
    niche: None,
};

fn scalar(bytes: usize, niche: Option<(u64, u64)>) -> Shape {
    Shape {
        bytes,
        inhabited: true,
        niche: niche.map(|(first, last)| Niche {
            offset: 0,
            width: bytes,
            first,
            last,
        }),
    }
}

fn record(fields: &[Shape], origin: bool) -> Shape {
    let mut shape = UNIT;
    for field in fields {
        shape.inhabited &= field.inhabited;
        if let Some(mut candidate) = field.niche {
            candidate.offset += shape.bytes;
            if shape
                .niche
                .is_none_or(|old| candidate.last - candidate.first > old.last - old.first)
            {
                shape.niche = Some(candidate);
            }
        }
        shape.bytes = shape.bytes.checked_add(field.bytes).unwrap();
    }
    if origin {
        shape.bytes = shape.bytes.checked_add(size_of::<u64>()).unwrap();
    }
    shape
}

fn plan(variants: &[Shape]) -> Planned {
    let live: Vec<_> = variants
        .iter()
        .filter(|variant| variant.inhabited)
        .collect();
    let shape = match live.as_slice() {
        [] => {
            return Planned {
                shape: Shape {
                    inhabited: false,
                    ..UNIT
                },
                encoding: Encoding::Empty,
            };
        }
        [only] => {
            return Planned {
                shape: **only,
                encoding: Encoding::Single,
            };
        }
        _ => live.iter().max_by_key(|variant| variant.bytes).unwrap(),
    };
    assert!(
        live.len() <= 256,
        "finite prototype supports byte discriminants"
    );
    let count = (live.len() - 1) as u64;
    if live.iter().filter(|variant| variant.bytes > 0).count() == 1
        && let Some(niche) = shape.niche
        && count <= niche.last - niche.first
        && niche.offset.checked_add(niche.width).unwrap() <= shape.bytes
    {
        return Planned {
            shape: Shape {
                niche: Some(Niche {
                    first: niche.first + count,
                    ..niche
                }),
                ..**shape
            },
            encoding: Encoding::Niche {
                offset: niche.offset,
                first: niche.first,
            },
        };
    }
    // The final niche value may be consumed exactly, leaving no remaining niche.
    if live.iter().filter(|variant| variant.bytes > 0).count() == 1
        && let Some(niche) = shape.niche
        && count == niche.last - niche.first + 1
    {
        return Planned {
            shape: Shape {
                niche: None,
                ..**shape
            },
            encoding: Encoding::Niche {
                offset: niche.offset,
                first: niche.first,
            },
        };
    }
    let niche = (live.len() < 256).then_some(Niche {
        offset: 0,
        width: 1,
        first: live.len() as u64,
        last: 255,
    });
    Planned {
        shape: Shape {
            bytes: shape.bytes + 1,
            inhabited: true,
            niche,
        },
        encoding: Encoding::Direct,
    }
}

fn optional(shape: Shape) -> Planned {
    plan(&[shape, UNIT])
}

enum Node {
    Leaf(Shape),
    Enum(Vec<Vec<usize>>),
}

fn reaches(node: usize, target: usize, graph: &[Node], seen: &mut HashSet<usize>) -> bool {
    if !seen.insert(node) {
        return false;
    }
    let Node::Enum(variants) = &graph[node] else {
        return false;
    };
    variants
        .iter()
        .flatten()
        .any(|&child| child == target || reaches(child, target, graph, seen))
}

fn graph_layout(node: usize, graph: &[Node]) -> Planned {
    let heap = || Planned {
        shape: scalar(12, None),
        encoding: Encoding::Heap,
    };
    // This finite probe checks SCC membership by reachability. Production should
    // use one bounded SCC pass, not repeat this per-node search.
    if reaches(node, node, graph, &mut HashSet::new()) {
        return heap();
    }
    let planned = match &graph[node] {
        Node::Leaf(shape) => Planned {
            shape: *shape,
            encoding: Encoding::Single,
        },
        Node::Enum(variants) => {
            let records: Vec<_> = variants
                .iter()
                .map(|fields| {
                    record(
                        &fields
                            .iter()
                            .map(|&field| graph_layout(field, graph).shape)
                            .collect::<Vec<_>>(),
                        false,
                    )
                })
                .collect();
            plan(&records)
        }
    };
    if planned.shape.bytes > INLINE_BYTES {
        heap()
    } else {
        planned
    }
}

fn decode_optional<T>(
    layout: &Planned,
    bits: u64,
    child: impl FnOnce(u64) -> Option<T>,
) -> Option<Option<T>> {
    match layout.encoding {
        Encoding::Niche { offset: 0, first } if bits == first => Some(None),
        Encoding::Niche { offset: 0, .. } => child(bits).map(Some),
        Encoding::Direct if bits & 255 == 1 => Some(None),
        Encoding::Direct if bits & 255 == 0 => child(bits >> 8).map(Some),
        _ => None,
    }
}

fn describe(name: &str, layout: &Planned, expected_bytes: usize) {
    assert_eq!(layout.shape.bytes, expected_bytes, "{name}");
    println!(
        "layout_model name={name} storage_bytes={} eligible={} encoding={:?} remaining_niche={:?}",
        layout.shape.bytes,
        layout.shape.bytes <= INLINE_BYTES,
        layout.encoding,
        layout.shape.niche,
    );
}

pub(super) fn run() {
    let boolean = scalar(1, Some((2, 255)));
    let character = scalar(4, Some((0x110000, u64::from(u32::MAX))));
    let integer = scalar(8, None);
    let managed = scalar(12, None);
    let absent = Shape {
        inhabited: false,
        ..UNIT
    };
    let optional_bool = optional(boolean);
    describe("Option<bool>", &optional_bool, 1);
    describe("Option<Option<bool>>", &optional(optional_bool.shape), 1);
    describe("Option<char>", &optional(character), 4);
    describe("Option<u64>", &optional(integer), 9);
    describe("Option<object>", &optional(managed), 13);
    let color = plan(&[UNIT, UNIT, UNIT]);
    describe("Color", &color, 1);
    describe("Option<Color>", &optional(color.shape), 1);
    let result = plan(&[integer, record(&[integer], true)]);
    describe("Result<u64,u64>", &result, 17);
    describe("Option<Result<u64,u64>>", &optional(result.shape), 17);
    describe(
        "Result<Result<u64,u64>,u64>",
        &plan(&[result.shape, record(&[integer], true)]),
        18,
    );
    describe(
        "two_object_single_variant",
        &plan(&[record(&[managed, managed], false)]),
        24,
    );
    let nested_origin = plan(&[integer, record(&[result.shape], true)]);
    describe("two_active_origins", &nested_origin, 26);
    describe("Option<Infallible>", &optional(absent), 0);
    describe("Infallible", &plan(&[]), 0);

    let graph = [
        Node::Enum(vec![vec![], vec![1, 0]]),
        Node::Leaf(integer),
        Node::Enum(vec![vec![0], vec![]]),
        Node::Enum(vec![vec![4]]),
        Node::Enum(vec![vec![3]]),
        Node::Enum(vec![vec![1, 1, 1, 1]]),
        Node::Enum(vec![vec![5], vec![]]),
    ];
    for node in [0, 3, 4, 5] {
        assert_eq!(graph_layout(node, &graph).encoding, Encoding::Heap);
    }
    describe("Option<recursive_enum>", &graph_layout(2, &graph), 13);
    describe("Option<oversized_enum>", &graph_layout(6, &graph), 13);

    // Independent finite-domain oracle: two bool values + one distinct state per layer.
    let mut nested = boolean;
    for depth in 1..=254 {
        nested = optional(nested).shape;
        assert_eq!(nested.bytes, 1, "depth {depth}");
        assert_eq!(
            nested.niche.map(|niche| niche.first),
            (depth < 254).then_some(depth + 2)
        );
    }
    describe("bool_niche_exhaustion", &optional(nested), 2);
    assert_eq!(optional(nested).encoding, Encoding::Direct);
    let nested_bool = optional(optional_bool.shape);
    for byte in 0u8..=255 {
        let decoded = decode_optional(&nested_bool, u64::from(byte), |inner| {
            decode_optional(&optional_bool, inner, |bits| match bits {
                0 => Some(false),
                1 => Some(true),
                _ => None,
            })
        });
        let expected = match byte {
            0 => Some(Some(Some(false))),
            1 => Some(Some(Some(true))),
            2 => Some(Some(None)),
            3 => Some(None),
            _ => None,
        };
        assert_eq!(decoded, expected, "nested bool byte {byte}");
    }
    let optional_char = optional(character);
    for bits in [0, 0xd7ff, 0xe000, 0x10ffff] {
        assert_eq!(
            decode_optional(&optional_char, u64::from(bits), |bits| char::from_u32(
                bits as u32
            )),
            Some(char::from_u32(bits))
        );
    }
    assert_eq!(
        decode_optional(&optional_char, 0x110000, |bits| char::from_u32(bits as u32)),
        Some(None)
    );
    for bits in [0xd800, 0xdfff, u32::MAX] {
        assert_eq!(
            decode_optional(&optional_char, u64::from(bits), |bits| char::from_u32(
                bits as u32
            )),
            None
        );
    }
    println!(
        "layout_model finite_domain_checks=passed packed_alignment=1 inline_budget={INLINE_BYTES}"
    );
}
