use crate::standard::RuntimePrimitive;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectSet {
    pub reads_local: bool,
    pub writes_local: bool,
    pub reads_module: bool,
    pub writes_module: bool,
    pub reads_aggregate: bool,
    pub writes_aggregate: bool,
    pub reads_path: bool,
    pub writes_path: bool,
    pub allocates: bool,
    pub calls: bool,
    pub touches_runtime: bool,
    pub may_trap: bool,
}

impl EffectSet {
    pub fn native_call() -> Self {
        Self {
            reads_module: true,
            writes_module: true,
            reads_path: true,
            writes_path: true,
            reads_aggregate: true,
            writes_aggregate: true,
            allocates: true,
            ..Self::runtime_call()
        }
    }
    pub fn union(self, other: Self) -> Self {
        Self {
            reads_local: self.reads_local || other.reads_local,
            writes_local: self.writes_local || other.writes_local,
            reads_module: self.reads_module || other.reads_module,
            writes_module: self.writes_module || other.writes_module,
            reads_aggregate: self.reads_aggregate || other.reads_aggregate,
            writes_aggregate: self.writes_aggregate || other.writes_aggregate,
            reads_path: self.reads_path || other.reads_path,
            writes_path: self.writes_path || other.writes_path,
            allocates: self.allocates || other.allocates,
            calls: self.calls || other.calls,
            touches_runtime: self.touches_runtime || other.touches_runtime,
            may_trap: self.may_trap || other.may_trap,
        }
    }

    pub fn local_read() -> Self {
        Self {
            reads_local: true,
            ..Self::default()
        }
    }

    pub fn local_write() -> Self {
        Self {
            writes_local: true,
            ..Self::default()
        }
    }

    pub fn aggregate_read() -> Self {
        Self {
            reads_aggregate: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn module_read() -> Self {
        Self {
            reads_module: true,
            touches_runtime: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn module_write() -> Self {
        Self {
            writes_module: true,
            touches_runtime: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn aggregate_write() -> Self {
        Self {
            writes_aggregate: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn path_read() -> Self {
        Self {
            reads_path: true,
            touches_runtime: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn path_write() -> Self {
        Self {
            writes_path: true,
            touches_runtime: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn allocation() -> Self {
        Self {
            allocates: true,
            touches_runtime: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn call() -> Self {
        Self {
            calls: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn runtime_call() -> Self {
        Self {
            calls: true,
            touches_runtime: true,
            may_trap: true,
            ..Self::default()
        }
    }
}

pub fn runtime_primitive_effects(primitive: RuntimePrimitive) -> EffectSet {
    let read = EffectSet::runtime_call().union(EffectSet::aggregate_read());
    match primitive {
        RuntimePrimitive::ValueEq | RuntimePrimitive::ValueHash | RuntimePrimitive::Assert => read,
        RuntimePrimitive::ValuePartialCmp
        | RuntimePrimitive::ValueCmp
        | RuntimePrimitive::ValueDebug
        | RuntimePrimitive::ValueDisplay
        | RuntimePrimitive::StringPartsJoin => read.union(EffectSet::allocation()),
    }
}
