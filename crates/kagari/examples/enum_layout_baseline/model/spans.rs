//! Typed backing records and temporary views avoid widening persistent non-enum slots.
//! Runtime root, callback and generation admission are not implemented by this model.
use kagari_runtime::{diagnostics::allocations, value::Value};
use std::{hint::black_box, mem::size_of};

#[derive(Clone, Copy)]
enum Domain {
    OnlyNone,
    NestedBool,
    OptionalInteger,
    ResultInteger,
}

struct Descriptor {
    identity: u64,
    domain: Domain,
    bytes: usize,
}

#[derive(Clone, Copy)]
struct View<'a> {
    descriptor: &'a Descriptor,
    bytes: &'a [u8],
}

struct Storage<'a> {
    descriptor: &'a Descriptor,
    data: Vec<u8>,
    count: usize,
}

impl View<'_> {
    fn valid(self) -> bool {
        self.bytes.len() == self.descriptor.bytes
            && match self.descriptor.domain {
                Domain::OnlyNone => self.bytes.is_empty(),
                Domain::NestedBool => matches!(self.bytes, [0..=3]),
                Domain::OptionalInteger => matches!(self.bytes.first(), Some(0 | 1)),
                Domain::ResultInteger => {
                    matches!(self.bytes.first(), Some(0))
                        || (matches!(self.bytes.first(), Some(1))
                            && self.bytes[9..17].iter().any(|byte| *byte != 0))
                }
            }
    }
}

impl<'a> Storage<'a> {
    fn new(descriptor: &'a Descriptor, count: usize) -> Self {
        Self {
            descriptor,
            data: vec![0; descriptor.bytes.checked_mul(count).unwrap()],
            count,
        }
    }

    fn view(&self, index: usize) -> Option<View<'_>> {
        if index >= self.count {
            return None;
        }
        let start = index.checked_mul(self.descriptor.bytes)?;
        let end = start.checked_add(self.descriptor.bytes)?;
        let view = View {
            descriptor: self.descriptor,
            bytes: self.data.get(start..end)?,
        };
        view.valid().then_some(view)
    }

    fn write(&mut self, index: usize, source: View<'_>) -> Option<()> {
        if index >= self.count
            || source.descriptor.identity != self.descriptor.identity
            || !source.valid()
        {
            return None;
        }
        let start = index.checked_mul(self.descriptor.bytes)?;
        let end = start.checked_add(self.descriptor.bytes)?;
        self.data.get_mut(start..end)?.copy_from_slice(source.bytes);
        Some(())
    }
}

pub(super) fn run() {
    const COUNT: usize = 1024;
    let none = Descriptor {
        identity: 5,
        domain: Domain::OnlyNone,
        bytes: 0,
    };
    let nested = Descriptor {
        identity: 1,
        domain: Domain::NestedBool,
        bytes: 1,
    };
    let integer = Descriptor {
        identity: 2,
        domain: Domain::OptionalInteger,
        bytes: 9,
    };
    let result = Descriptor {
        identity: 3,
        domain: Domain::ResultInteger,
        bytes: 17,
    };
    let foreign = Descriptor {
        identity: 4,
        domain: Domain::OptionalInteger,
        bytes: 9,
    };
    for descriptor in [&none, &nested, &integer, &result] {
        let mut source = Storage::new(descriptor, COUNT);
        let mut target = Storage::new(descriptor, COUNT);
        if matches!(descriptor.domain, Domain::ResultInteger) {
            source.data[0] = 1;
            source.data[9..17].copy_from_slice(&23u64.to_ne_bytes());
        }
        let ((), requests) = allocations::measure(|| {
            for index in 0..COUNT {
                target.write(index, source.view(index).unwrap()).unwrap();
                black_box(target.view(index).unwrap());
            }
        });
        assert_eq!(requests.requests, 0, "warmed typed record copies");
        assert_eq!(target.data, source.data);
        assert!(target.view(usize::MAX).is_none());
        assert!(
            target.view(COUNT).is_none(),
            "zero-sized storage still has a logical length"
        );
        assert!(target.write(usize::MAX, source.view(0).unwrap()).is_none());
        println!(
            "span_model element_bytes={} count={COUNT} backing_bytes={} copy_allocations={}",
            descriptor.bytes,
            target.data.len(),
            requests.requests,
        );
    }
    let source = Storage::new(&integer, 1);
    let mut target = Storage::new(&foreign, 1);
    assert!(target.write(0, source.view(0).unwrap()).is_none());
    let mut invalid = Storage::new(&nested, 1);
    invalid.data[0] = 4;
    assert!(invalid.view(0).is_none());
    let mut invalid = Storage::new(&result, 1);
    invalid.data[0] = 1;
    assert!(
        invalid.view(0).is_none(),
        "Err requires a nonzero origin token"
    );
    println!(
        "span_model view_bytes={} persistent_non_enum_Value={} nominal_rejection=passed bounds=passed",
        size_of::<View<'_>>(),
        size_of::<Value>(),
    );
}
