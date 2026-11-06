use gc_arena::{Arena, Gc, Rootable, arena::CollectionPhase};
use kagari_gc::{
    error::GcError,
    heap::{HeapConfig, Phase},
    root_source::RootDriver,
};
use std::{hint::black_box, time::Instant};

use crate::{Engine, ITERATIONS, elapsed, frames::Frames};

pub fn copies(engine: Engine) -> Result<u128, GcError> {
    match engine {
        Engine::Kagari => kagari_copies(),
        Engine::Arena => Ok(arena_copies()),
    }
}

fn kagari_copies() -> Result<u128, GcError> {
    let mut driver = RootDriver::<Frames<u64>>::new(HeapConfig::default())?;
    let id = driver.scope(|cx, roots| {
        let a = cx.alloc_leaf(7u64)?;
        let b = cx.alloc_leaf(11u64)?;
        let id = Frames::enter(roots, cx, 3)?;
        Frames::with_frame(roots, cx, id, |frame| {
            frame.grow(32);
            frame.set(0, Some(a))?;
            frame.set(2, Some(b))
        })?;
        Ok(id)
    })?;
    assert_eq!(driver.stats().phase, Phase::Idle);
    let ns = driver.scope(|cx, roots| {
        let ns = Frames::with_frame(roots, cx, id, |frame| {
            let start = Instant::now();
            for i in 0..ITERATIONS {
                black_box(&mut *frame).copy(black_box(1), black_box((i & 1) * 2))?;
            }
            Ok(elapsed(start))
        })?;
        let value = Frames::with_frame(roots, cx, id, |frame| frame.get(1))?.unwrap();
        assert_eq!(cx.read(value, |v| *v), 11);
        Ok(ns)
    })?;
    driver.collect()?;
    assert_eq!(driver.stats().objects, 2);
    driver.scope(|cx, roots| Frames::leave(roots, cx, id))?;
    driver.collect()?;
    assert_eq!(driver.stats().objects, 0);
    assert_eq!(driver.stats().backing_bytes, 0);
    Ok(ns)
}

fn arena_copies() -> u128 {
    let mut arena = Arena::<Rootable![Vec<Option<Gc<'_, u64>>>]>::new(|mc| {
        let mut roots = vec![None; 32];
        roots[0] = Some(Gc::new(mc, 7));
        roots[2] = Some(Gc::new(mc, 11));
        roots
    });
    assert_eq!(arena.collection_phase(), CollectionPhase::Sleeping);
    let ns = arena.mutate_root(|_, roots| {
        let start = Instant::now();
        for i in 0..ITERATIONS {
            let roots = black_box(&mut *roots);
            let value = roots[black_box((i & 1) * 2)];
            roots[black_box(1)] = value;
        }
        let ns = elapsed(start);
        assert_eq!(*roots[1].unwrap(), 11);
        ns
    });
    arena.finish_cycle();
    assert_eq!(arena.metrics().total_gc_count(), 2);
    arena.mutate_root(|_, roots| roots.fill(None));
    arena.finish_cycle();
    assert_eq!(arena.metrics().total_gc_count(), 0);
    ns
}

pub fn population(
    engine: Engine,
    count: usize,
    prepared: bool,
) -> Result<[(&'static str, u128); 4], GcError> {
    let times = match engine {
        Engine::Kagari if prepared => kagari_population::<true>(count)?,
        Engine::Kagari => kagari_population::<false>(count)?,
        Engine::Arena => arena_population(count),
    };
    Ok([
        (
            if prepared && matches!(engine, Engine::Kagari) {
                "allocate_prepared"
            } else {
                "allocate_rooted"
            },
            times[0],
        ),
        ("collect_live", times[1]),
        ("clear_roots", times[2]),
        ("collect_dead", times[3]),
    ])
}

fn kagari_population<const PREPARED: bool>(count: usize) -> Result<[u128; 4], GcError> {
    let mut driver = RootDriver::<Frames<u64>>::new(HeapConfig::default())?;
    let id = driver.scope(|cx, roots| Frames::enter(roots, cx, count))?;
    // Prepared mode measures registration separately; normal mode includes first
    // registration and per-allocation type lookup, as in the original fixture.
    let descriptor = if PREPARED {
        Some(driver.access()?.register::<u64>()?)
    } else {
        None
    };
    let allocation = driver.scope(|cx, roots| {
        let start = Instant::now();
        for i in 0..count {
            let value = match &descriptor {
                Some(descriptor) => descriptor.alloc_leaf(cx, black_box(i as u64))?,
                None => cx.alloc_leaf(black_box(i as u64))?,
            };
            // Current safe adapter admits the frame per allocation/store pair.
            Frames::with_frame(roots, cx, id, |frame| frame.set(i, Some(value)))?;
        }
        Ok(elapsed(start))
    })?;
    assert_eq!(driver.stats().phase, Phase::Idle);
    let start = Instant::now();
    driver.collect()?;
    let live = elapsed(start);
    assert_eq!(driver.stats().objects, count);
    assert_eq!(driver.stats().root_registrations, 0);
    driver.scope(|cx, roots| {
        for i in 0..count {
            let value = Frames::with_frame(roots, cx, id, |frame| frame.get(i))?.unwrap();
            assert_eq!(cx.read(value, |v| *v), i as u64);
        }
        Ok(())
    })?;
    let clear = driver.scope(|cx, roots| {
        Frames::with_frame(roots, cx, id, |frame| {
            let start = Instant::now();
            for i in 0..count {
                frame.set(i, None)?;
            }
            Ok(elapsed(start))
        })
    })?;
    let start = Instant::now();
    driver.collect()?;
    let dead = elapsed(start);
    assert_eq!(driver.stats().objects, 0);
    assert_eq!(driver.stats().backing_bytes, 0);
    driver.scope(|cx, roots| Frames::leave(roots, cx, id))?;
    Ok([allocation, live, clear, dead])
}

fn arena_population(count: usize) -> [u128; 4] {
    let mut arena = Arena::<Rootable![Vec<Option<Gc<'_, u64>>>]>::new(|_| vec![None; count]);
    let allocation = arena.mutate_root(|mc, roots| {
        let start = Instant::now();
        for (i, slot) in roots.iter_mut().enumerate() {
            *slot = Some(Gc::new(mc, black_box(i as u64)));
        }
        elapsed(start)
    });
    assert_eq!(arena.collection_phase(), CollectionPhase::Sleeping);
    let start = Instant::now();
    arena.finish_cycle();
    let live = elapsed(start);
    assert_eq!(arena.metrics().total_gc_count(), count);
    arena.mutate(|_, roots| {
        for (i, slot) in roots.iter().enumerate() {
            assert_eq!(*slot.unwrap(), i as u64);
        }
    });
    let clear = arena.mutate_root(|_, roots| {
        let start = Instant::now();
        roots.fill(None);
        elapsed(start)
    });
    let start = Instant::now();
    arena.finish_cycle();
    let dead = elapsed(start);
    assert_eq!(arena.metrics().total_gc_count(), 0);
    [allocation, live, clear, dead]
}
