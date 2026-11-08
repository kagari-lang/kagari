//! Mark/reclaim proof for Copy carriers; not a replacement for Runtime integration.
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

static NEXT: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Token(u64);

fn token() -> Token {
    Token(
        NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
            next.checked_add(1)
        })
        .unwrap(),
    )
}

#[derive(Debug)]
struct Descriptor {
    installation: u64,
    origins: Vec<OriginPath>,
}

#[derive(Debug)]
struct OriginPath {
    offset: usize,
    conditions: Vec<(usize, u8)>,
}

#[derive(Debug)]
struct Origin {
    source: &'static str,
}

#[derive(Clone, Copy)]
struct Carrier {
    descriptor: Token,
    storage: [u8; 24],
}

#[derive(Default)]
struct Store {
    descriptors: HashMap<Token, Arc<Descriptor>>,
    origins: HashMap<Token, Arc<Origin>>,
}

impl Store {
    fn descriptor(&mut self, value: Arc<Descriptor>) -> Token {
        let key = token();
        self.descriptors.insert(key, value);
        key
    }

    fn origin(&mut self, value: Arc<Origin>) -> Token {
        let key = token();
        self.origins.insert(key, value);
        key
    }

    fn collect(&mut self, roots: &[Carrier]) -> Option<()> {
        let mut layouts = HashSet::new();
        let mut origins = HashSet::new();
        // Validate the entire admitted root pack before mutating either table.
        for root in roots {
            let descriptor = self.descriptors.get(&root.descriptor)?;
            layouts.insert(root.descriptor);
            if root.storage[0] > 1 || (root.storage[0] == 1 && root.storage[1] > 1) {
                return None;
            }
            for path in &descriptor.origins {
                if !path
                    .conditions
                    .iter()
                    .all(|&(offset, tag)| root.storage.get(offset) == Some(&tag))
                {
                    continue;
                }
                let bytes = root.storage.get(path.offset..path.offset.checked_add(8)?)?;
                let key = Token(u64::from_ne_bytes(bytes.try_into().ok()?));
                let _ = self.origins.get(&key)?;
                origins.insert(key);
            }
        }
        self.descriptors.retain(|key, _| layouts.contains(key));
        self.origins.retain(|key, _| origins.contains(key));
        Some(())
    }
}

pub(super) fn run() {
    let mut store = Store::default();
    let descriptor = Arc::new(Descriptor {
        installation: 17,
        origins: vec![
            OriginPath {
                offset: 2,
                conditions: vec![(0, 1)],
            },
            OriginPath {
                offset: 10,
                conditions: vec![(0, 1), (1, 1)],
            },
        ],
    });
    let descriptor_weak = Arc::downgrade(&descriptor);
    let key = store.descriptor(descriptor);
    let outer = Arc::new(Origin { source: "outer" });
    let inner = Arc::new(Origin { source: "inner" });
    let outer_weak = Arc::downgrade(&outer);
    let inner_weak = Arc::downgrade(&inner);
    let outer_key = store.origin(outer);
    let inner_key = store.origin(inner);
    let mut root = Carrier {
        descriptor: key,
        storage: [0; 24],
    };
    root.storage[0..2].copy_from_slice(&[1, 1]);
    root.storage[2..10].copy_from_slice(&outer_key.0.to_ne_bytes());
    root.storage[10..18].copy_from_slice(&inner_key.0.to_ne_bytes());
    let copied = root;
    store.collect(&[copied]).unwrap();
    assert_eq!(descriptor_weak.upgrade().unwrap().installation, 17);
    assert_eq!(outer_weak.upgrade().unwrap().source, "outer");
    assert_eq!(inner_weak.upgrade().unwrap().source, "inner");
    assert_eq!(store.origins.len(), 2);
    assert!(Store::default().collect(&[copied]).is_none());
    let mut forged = copied;
    forged.storage[10..18].fill(0);
    assert!(store.collect(&[forged]).is_none());
    assert_eq!(
        store.origins.len(),
        2,
        "invalid root must not cause partial sweep"
    );
    let inactive = Carrier {
        descriptor: key,
        storage: [0; 24],
    };
    store.collect(&[inactive]).unwrap();
    assert!(descriptor_weak.upgrade().is_some());
    assert!(outer_weak.upgrade().is_none());
    assert!(inner_weak.upgrade().is_none());
    store.collect(&[]).unwrap();
    assert!(descriptor_weak.upgrade().is_none());
    assert!(store.collect(&[copied]).is_none(), "stale tokens must fail");
    let fresh = store.descriptor(Arc::new(Descriptor {
        installation: 18,
        origins: vec![],
    }));
    assert_ne!(fresh, key, "reclaimed identities are never reused");
    let none = Carrier {
        descriptor: fresh,
        storage: [0; 24],
    };
    store.collect(&[none]).unwrap();
    assert_eq!(store.descriptors[&fresh].installation, 18);
    store.collect(&[]).unwrap();
    assert!(store.descriptors.is_empty());
    println!(
        "retention_model checked=copy,two_origins,foreign,stale,partial_sweep,inactive,none_provenance,reclaim token_bytes=8"
    );
}
