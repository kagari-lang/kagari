//! Deterministic payload-only occupancy model, not an allocator or pause prediction.
use crate::native_layouts;
use kagari_runtime::value::Value;
use std::{
    collections::BTreeMap,
    mem::{align_of, size_of},
};

const PAGE: usize = 4096;
const STRIDES: [usize; 24] = [
    8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640,
    768, 896, 1024,
];

struct Allocation {
    size: usize,
    align: usize,
    scan: bool,
    nominal: usize,
    keep: bool,
}

#[derive(Default)]
struct Page {
    allocated: usize,
    live: usize,
    live_payload: usize,
}

fn class(size: usize, align: usize) -> Option<usize> {
    assert!(align.is_power_of_two());
    (align <= 16)
        .then(|| {
            STRIDES
                .into_iter()
                .find(|stride| *stride >= size.max(1) && stride % align == 0)
        })
        .flatten()
}

fn report(name: &str, allocations: &[Allocation], segregate_nominal: bool) {
    let mut pools: BTreeMap<(usize, bool, usize), Vec<Page>> = BTreeMap::new();
    let mut separate_peak = 0;
    let mut separate_live = 0;
    let mut separate_count = 0;
    let mut requested_live = 0;
    for allocation in allocations {
        if allocation.keep {
            requested_live += allocation.size;
        }
        let Some(stride) = class(allocation.size, allocation.align) else {
            separate_count += 1;
            separate_peak += allocation.size;
            if allocation.keep {
                separate_live += allocation.size;
            }
            continue;
        };
        let key = (
            stride,
            allocation.scan,
            if segregate_nominal {
                allocation.nominal
            } else {
                0
            },
        );
        let pages = pools.entry(key).or_default();
        if pages
            .last()
            .is_none_or(|page| page.allocated == PAGE / stride)
        {
            pages.push(Page::default());
        }
        let page = pages.last_mut().unwrap();
        page.allocated += 1;
        if allocation.keep {
            page.live += 1;
            page.live_payload += allocation.size;
        }
    }
    let mut peak_pages = 0;
    let mut live_pages = 0;
    let mut rounding = 0;
    let mut vacant = 0;
    let mut tails = 0;
    for ((stride, _, _), pages) in &pools {
        peak_pages += pages.len();
        for page in pages.iter().filter(|page| page.live != 0) {
            live_pages += 1;
            rounding += page.live * stride - page.live_payload;
            vacant += (PAGE / stride - page.live) * stride;
            tails += PAGE % stride;
        }
    }
    let live_backing = live_pages * PAGE + separate_live;
    assert_eq!(live_backing, requested_live + rounding + vacant + tails);
    println!(
        "slab_model,{name},nominal_pools={segregate_nominal},allocations={},peak_pages={peak_pages},live_pages={live_pages},released_pages={},peak_backing_bytes={},live_backing_bytes={live_backing},live_payload_bytes={requested_live},rounding_bytes={rounding},vacant_slot_bytes={vacant},tail_bytes={tails},separate_allocations={separate_count},separate_peak_bytes={separate_peak},separate_live_bytes={separate_live},empty_backing_bytes=0",
        allocations.len(),
        peak_pages - live_pages,
        peak_pages * PAGE + separate_peak,
    );
}

fn case(name: &str, allocations: Vec<Allocation>) {
    report(name, &allocations, false);
    if name == "sparse_types_32x4" {
        report(name, &allocations, true);
    }
    let empty: Vec<_> = allocations
        .into_iter()
        .map(|allocation| Allocation {
            keep: false,
            ..allocation
        })
        .collect();
    report(&format!("{name}_empty"), &empty, false);
}

pub(super) fn run() {
    println!(
        "slab_assumptions,page_bytes=4096,page_alignment=16,maximum_pooled_alignment=16,maximum_stride=1024,grouping=stride+scan,placement=allocation_order,no_survivor_moves=true,empty_release=immediate,headers_descriptors_directories_bitmaps_allocator_overhead=excluded"
    );
    for stride in STRIDES {
        println!(
            "slab_class,stride={stride},slots={},tail_bytes={}",
            PAGE / stride,
            PAGE % stride
        );
    }
    // Exhaustively check the small finite size/alignment domain, not runtime workloads.
    let mut worst_rounding = (0, 0, 0);
    for align in [1, 2, 4, 8, 16] {
        for size in (align..=1024).step_by(align) {
            let stride = class(size, align).unwrap();
            assert!(stride >= size && stride.is_multiple_of(align));
            if stride - size > worst_rounding.0 {
                worst_rounding = (stride - size, size, align);
            }
        }
    }
    assert_eq!(class(1025, 1), None);
    assert_eq!(class(64, 64), None);
    assert_eq!(class(24, 16), Some(32));
    println!(
        "slab_rounding,max_absolute_bytes={},size={},align={}",
        worst_rounding.0, worst_rounding.1, worst_rounding.2
    );
    case(
        "sparse_types_32x4",
        (0..128)
            .map(|index| Allocation {
                size: 8,
                align: 4,
                scan: false,
                nominal: index % 32,
                keep: true,
            })
            .collect(),
    );
    case(
        "dense_points_8192",
        (0..8192)
            .map(|_| Allocation {
                size: 8,
                align: 4,
                scan: false,
                nominal: 0,
                keep: true,
            })
            .collect(),
    );
    case(
        "sparse_nonmoving_4096_keep64",
        (0..4096)
            .map(|index| Allocation {
                size: 24,
                align: 8,
                scan: true,
                nominal: 0,
                keep: index % 64 == 0,
            })
            .collect(),
    );
    case(
        "mixed_native_128x6",
        (0..128)
            .flat_map(|_| {
                native_layouts::layouts().into_iter().enumerate().map(
                    |(nominal, (_, size, align))| Allocation {
                        size,
                        align,
                        scan: false,
                        nominal,
                        keep: true,
                    },
                )
            })
            .collect(),
    );
    case(
        "large_buffers",
        vec![
            Allocation {
                size: 65_536 * size_of::<i32>(),
                align: align_of::<i32>(),
                scan: false,
                nominal: 0,
                keep: true,
            },
            Allocation {
                size: 8192 * size_of::<Value>(),
                align: align_of::<Value>(),
                scan: true,
                nominal: 1,
                keep: true,
            },
        ],
    );
}
