//! Ordinary embedding boundaries; the scripts own allocation and graph traversal.
use crate::Case;
use kagari::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
    runtime::Runtime,
};
use kagari_runtime::{module::Program, native::value_handle::ScriptValue};
use kagari_source::source::SourceFile;
use mlua::{
    Function, Lua, Table,
    state::{GcIncParams, GcMode},
};

pub trait Engine {
    type Root;

    fn construct(&mut self, count: i32) -> Self::Root;

    fn check(&mut self, root: &Self::Root, stride: i32, expected: i32);

    fn keep(&mut self, root: &Self::Root) -> Self::Root;

    fn collect(&mut self);

    /// Exact heap occupancy is exposed by Kagari, not Lua's public API.
    fn live_objects(&self) -> Option<usize>;

    fn collections(&self) -> Option<u64>;
}

pub struct KagariFactory {
    engine: KagariEngine,
    program: PreparedProgram,
}

impl KagariFactory {
    pub fn new(automatic: bool) -> Self {
        let mut config = EngineConfig::default();
        if !automatic {
            config.default_runtime.gc.collection_threshold = None;
        }
        let engine = KagariEngine::new(config);
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("gc-comparison.kgr", include_str!("graph.kgr")),
                Default::default(),
            )
            .unwrap();
        let program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        Self { engine, program }
    }

    pub fn create(&self, case: Case) -> Kagari {
        let context = ExecutionContext::default();
        let mut runtime = self.engine.runtime(context.clone());
        let loaded = runtime
            .load_program(&self.program, Default::default())
            .unwrap();
        Kagari {
            runtime,
            loaded,
            context,
            case,
        }
    }
}

pub struct Kagari {
    runtime: Runtime,
    loaded: Program,
    context: ExecutionContext,
    case: Case,
}

impl Engine for Kagari {
    type Root = ScriptValue;

    fn construct(&mut self, count: i32) -> Self::Root {
        self.runtime
            .execute_typed(&self.loaded, self.case.entry, (count,), &self.context)
            .unwrap()
    }

    fn check(&mut self, root: &Self::Root, stride: i32, expected: i32) {
        let actual: i32 = self
            .runtime
            .execute_typed(
                &self.loaded,
                self.case.check,
                (root.clone(), stride),
                &self.context,
            )
            .unwrap();
        assert_eq!(actual, expected);
    }

    fn keep(&mut self, root: &Self::Root) -> Self::Root {
        self.runtime
            .execute_typed(&self.loaded, self.case.keep, (root.clone(),), &self.context)
            .unwrap()
    }

    fn collect(&mut self) {
        self.runtime.runtime().collect_garbage().unwrap();
    }

    fn live_objects(&self) -> Option<usize> {
        Some(self.runtime.runtime().gc().stats().allocated_objects)
    }

    fn collections(&self) -> Option<u64> {
        Some(self.runtime.runtime().gc().stats().collections)
    }
}

pub struct LuaEngine {
    lua: Lua,
    construct: Function,
    check: Function,
    keep: Function,
}

impl LuaEngine {
    pub fn new(case: Case, automatic: bool) -> Self {
        let lua = Lua::new();
        // Explicitly select PUC 5.4 incremental defaults, not generational or LuaJIT.
        lua.gc_set_mode(GcMode::Incremental(
            GcIncParams::default()
                .pause(200)
                .step_multiplier(100)
                .step_size(13),
        ));
        let api: Table = lua.load(include_str!("graph.lua")).eval().unwrap();
        let construct = api.get(case.entry).unwrap();
        let check = api.get(case.check).unwrap();
        let keep = api.get(case.keep).unwrap();
        drop(api);
        if !automatic {
            lua.gc_stop();
        }
        Self {
            lua,
            construct,
            check,
            keep,
        }
    }
}

impl Engine for LuaEngine {
    type Root = Table;

    fn construct(&mut self, count: i32) -> Self::Root {
        self.construct.call(count).unwrap()
    }

    fn check(&mut self, root: &Self::Root, stride: i32, expected: i32) {
        assert_eq!(
            self.check.call::<i32>((root.clone(), stride)).unwrap(),
            expected
        );
    }

    fn keep(&mut self, root: &Self::Root) -> Self::Root {
        self.keep.call(root.clone()).unwrap()
    }

    fn collect(&mut self) {
        self.lua.gc_collect().unwrap();
    }

    fn live_objects(&self) -> Option<usize> {
        None
    }

    fn collections(&self) -> Option<u64> {
        None
    }
}
