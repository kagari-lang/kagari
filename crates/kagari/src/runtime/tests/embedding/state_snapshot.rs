//! UP03 composition only: independent runtimes, not quiescence or a routing cutover.
use crate::{engine::KagariEngine, program::PreparedProgram, runtime::Runtime};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_runtime::{
    module::Program,
    native::{
        conversion::context::ConversionContext, function_handle::Function, objects::Object,
        value_handle::ScriptValue,
    },
    state::{
        self,
        data::{SchemaVersion, SnapshotHeader, StateSchema, StateSnapshot, StateValue},
        error::{SnapshotErrorKind, SnapshotResult},
        limits::SnapshotOptions,
    },
};
use kagari_source::source::SourceFile;

const V1: &str = r#"
pub struct LiveState {
    val player_id: u64,
    var coins: i32,
    val callback: fn() -> i32,
}
pub struct StateDto {
    pub val player_id: u64,
    pub val coins: i32,
}
static var COUNT: i32 = 5;
static var INITIALIZATIONS: i32 = 0;
static var CALLBACK_RUNS: i32 = 0;
static val UNRELATED: i32 = initialize_unrelated();
fn initialize_unrelated() -> i32 { INITIALIZATIONS += 1; 99 }
pub fn unrelated() -> i32 { UNRELATED }
pub fn next() -> i32 { COUNT += 1; COUNT }
pub fn counters() -> (i32, i32, i32) { (COUNT, INITIALIZATIONS, CALLBACK_RUNS) }
pub fn make(player_id: u64) -> LiveState {
    LiveState { player_id, coins: 23, callback: || { CALLBACK_RUNS += 1; 700 } }
}
pub fn callback(root: LiveState) -> fn() -> i32 { root.callback }
pub fn earn(root: LiveState) -> i32 { root.coins += 1; root.coins }
pub fn balance(root: LiveState) -> i32 { root.coins }
pub fn export_state(root: LiveState) -> Result<StateDto, String> {
    Ok(StateDto { player_id: root.player_id, coins: root.coins })
}
pub fn restore_state(dto: StateDto) -> Result<LiveState, String> {
    Ok(LiveState {
        player_id: dto.player_id, coins: dto.coins,
        callback: || { CALLBACK_RUNS += 1; 700 },
    })
}
pub fn validate_state(root: LiveState) -> Result<(), String> {
    if root.player_id != 42 { Err("unknown player ID") } else { Ok(()) }
}
"#;

fn v2() -> String {
    V1.replace(
        "pub val coins: i32,",
        "pub val coins: i32, pub val bonus: i32,",
    )
    .replace("coins: root.coins }", "coins: root.coins, bonus: 0 }")
    .replace("coins: dto.coins,", "coins: dto.coins + dto.bonus,")
}

struct Fixture {
    runtime: Runtime,
    program: Program,
}

impl Fixture {
    fn source(source: &str) -> Self {
        let engine = KagariEngine::default();
        let prepared = engine
            .compile_to_program(
                SourceFile::new("memory://state-snapshot.kgr", source),
                Default::default(),
            )
            .unwrap();
        Self::install(prepared)
    }

    fn decoded(source: &str) -> Self {
        // Compile independently, then admit only serialized executable input.
        let engine = KagariEngine::default();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("memory://state-snapshot.kgr", source),
                Default::default(),
            )
            .unwrap();
        let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
        let decoded = KbcArtifact::from_bytes(&bytes).unwrap();
        let prepared =
            PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default())
                .unwrap();
        Self::install(prepared)
    }

    fn install(prepared: PreparedProgram) -> Self {
        let engine = KagariEngine::default();
        let mut runtime = engine.runtime(Default::default());
        let program = runtime.load_program(&prepared, Default::default()).unwrap();
        Self { runtime, program }
    }

    fn make(&mut self, player_id: u64) -> Object {
        let make = self
            .runtime
            .function::<(u64,), Object>(&self.program.root(), "make")
            .unwrap();
        self.runtime.call(&make, (player_id,)).unwrap()
    }

    fn callback(&mut self, root: &Object) -> Function<(), i32> {
        let callback = self
            .runtime
            .function::<(Object,), Function<(), i32>>(&self.program.root(), "callback")
            .unwrap();
        self.runtime.call(&callback, (root.clone(),)).unwrap()
    }

    fn integer(&mut self, name: &str) -> i32 {
        let function = self
            .runtime
            .function::<(), i32>(&self.program.root(), name)
            .unwrap();
        self.runtime.call(&function, ()).unwrap()
    }

    fn balance(&mut self, root: &Object, name: &str) -> i32 {
        let function = self
            .runtime
            .function::<(Object,), i32>(&self.program.root(), name)
            .unwrap();
        self.runtime.call(&function, (root.clone(),)).unwrap()
    }

    fn counters(&mut self) -> (i32, i32, i32) {
        let function = self
            .runtime
            .function::<(), (i32, i32, i32)>(&self.program.root(), "counters")
            .unwrap();
        self.runtime.call(&function, ()).unwrap()
    }

    fn export(&mut self, root: &Object) -> StateSnapshot {
        let project = self
            .runtime
            .function::<(Object,), Result<ScriptValue, String>>(
                &self.program.root(),
                "export_state",
            )
            .unwrap();
        let dto = self
            .runtime
            .call(&project, (root.clone(),))
            .unwrap()
            .unwrap();
        let owner = self
            .runtime
            .runtime()
            .select_program(&self.program)
            .unwrap();
        let mut cx = ConversionContext::new(self.runtime.runtime(), &owner).unwrap();
        state::export(&mut cx, &dto, header(), &SnapshotOptions::default()).unwrap()
    }

    fn schema(&self, version: u32) -> StateSchema {
        let expected = self
            .runtime
            .object_type(&self.program.root(), "StateDto")
            .unwrap();
        let mut cx = ConversionContext::new(self.runtime.runtime(), expected.owner()).unwrap();
        state::describe(
            &mut cx,
            expected.type_argument(),
            schema_version(version),
            &SnapshotOptions::default(),
        )
        .unwrap()
    }

    fn restore_dto(&self, snapshot: &StateSnapshot, version: u32) -> SnapshotResult<ScriptValue> {
        let expected = self
            .runtime
            .object_type(&self.program.root(), "StateDto")
            .unwrap();
        let mut cx = ConversionContext::new(self.runtime.runtime(), expected.owner()).unwrap();
        state::restore(
            &mut cx,
            expected.type_argument(),
            snapshot,
            &schema_version(version),
            &SnapshotOptions::default(),
        )
    }

    fn restore_live(&mut self, dto: ScriptValue) -> Object {
        let restore = self
            .runtime
            .function::<(ScriptValue,), Result<Object, String>>(
                &self.program.root(),
                "restore_state",
            )
            .unwrap();
        self.runtime.call(&restore, (dto,)).unwrap().unwrap()
    }

    fn validate(&mut self, root: &Object) -> Result<(), String> {
        let validate = self
            .runtime
            .function::<(Object,), Result<(), String>>(&self.program.root(), "validate_state")
            .unwrap();
        self.runtime.call(&validate, (root.clone(),)).unwrap()
    }
}

fn schema_version(version: u32) -> SchemaVersion {
    SchemaVersion::new("player-state", version).unwrap()
}

fn header() -> SnapshotHeader {
    SnapshotHeader {
        schema: schema_version(1),
        source_deployment: "deployment-v1".into(),
        state_owner: "player-42".into(),
        state_revision: 17,
    }
}

fn through_bytes(snapshot: &StateSnapshot) -> StateSnapshot {
    let options = SnapshotOptions::default();
    let bytes = snapshot.to_bytes(&options).unwrap();
    StateSnapshot::from_bytes(&bytes, &options).unwrap()
}

#[test]
fn unchanged_dto_restores_into_independent_decoded_program_with_fresh_statics() {
    let mut old = Fixture::source(V1);
    let live = old.make(42);
    let dormant = old.callback(&live);
    assert_eq!(old.integer("next"), 6);
    assert_eq!(old.balance(&live, "earn"), 24);
    assert_eq!(old.counters(), (6, 0, 0));

    let snapshot = through_bytes(&old.export(&live));
    assert_eq!(snapshot.header(), &header());
    let StateValue::Record { fields, .. } = snapshot.payload() else {
        panic!("projection must return a DTO record");
    };
    assert_eq!(
        fields,
        &vec![
            ("coins".into(), StateValue::I32(24)),
            ("player_id".into(), StateValue::U64(42)),
        ]
    );
    assert_eq!(old.counters(), (6, 0, 0));

    let mut candidate = Fixture::decoded(
        &V1.replace("COUNT: i32 = 5", "COUNT: i32 = 50")
            .replace("700", "900"),
    );
    assert_eq!(snapshot.schema(), &candidate.schema(1));
    let dto = candidate.restore_dto(&snapshot, 1).unwrap();
    let restored = candidate.restore_live(dto);
    assert_eq!(candidate.validate(&restored), Ok(()));
    assert_eq!(candidate.balance(&restored, "balance"), 24);
    assert_eq!(candidate.counters(), (50, 0, 0));
    assert_eq!(candidate.integer("next"), 51);
    assert_eq!(candidate.integer("unrelated"), 99);
    assert_eq!(candidate.counters(), (51, 1, 0));
    assert_eq!(old.counters(), (6, 0, 0));

    // The projection excluded behavior. It neither invoked nor retired old callbacks.
    assert_eq!(old.runtime.call(&dormant, ()).unwrap(), 700);
    let rebuilt = candidate.callback(&restored);
    assert_eq!(candidate.runtime.call(&rebuilt, ()).unwrap(), 900);
    assert_eq!(candidate.balance(&restored, "earn"), 25);
    assert_eq!(old.balance(&live, "balance"), 24);
    assert_eq!(old.integer("unrelated"), 99);
    assert_eq!(old.counters(), (6, 1, 1));
}

#[test]
fn changed_dto_requires_versioned_migration_with_an_explicit_default() {
    let mut old = Fixture::source(V1);
    let live = old.make(42);
    let snapshot = through_bytes(&old.export(&live));
    let mut candidate = Fixture::decoded(&v2());
    let target = candidate.schema(2);
    let options = SnapshotOptions::default();

    assert_eq!(
        candidate.restore_dto(&snapshot, 1).unwrap_err().kind,
        SnapshotErrorKind::SchemaMismatch
    );
    let failed = snapshot
        .clone()
        .migrate(&schema_version(1), &target, &options, |cx, value| {
            cx.check_cancelled()?;
            Ok(value)
        });
    assert_eq!(failed.unwrap_err().kind, SnapshotErrorKind::MissingMember);
    assert_eq!(old.validate(&live), Ok(()));
    assert_eq!(old.balance(&live, "earn"), 24);

    let migrated = snapshot
        .migrate(&schema_version(1), &target, &options, |cx, value| {
            assert_eq!(cx.source(), &header());
            assert_eq!(cx.target().version, schema_version(2));
            cx.charge_work(1)?;
            let StateValue::Record { key, mut fields } = value else {
                panic!("migration expects the v1 DTO");
            };
            // v2 explicitly grants five bonus coins; decoding supplies no default.
            fields.insert(0, (cx.copy_string("bonus")?, StateValue::I32(5)));
            Ok(StateValue::Record { key, fields })
        })
        .unwrap();
    let migrated = through_bytes(&migrated);
    assert_eq!(
        migrated.header(),
        &SnapshotHeader {
            schema: schema_version(2),
            ..header()
        }
    );
    let restored = candidate.restore_live(candidate.restore_dto(&migrated, 2).unwrap());
    assert_eq!(candidate.validate(&restored), Ok(()));
    assert_eq!(candidate.balance(&restored, "balance"), 28);
    assert_eq!(old.balance(&live, "balance"), 24);
}

#[test]
fn structurally_valid_business_id_rejection_leaves_old_runtime_usable() {
    let mut old = Fixture::source(V1);
    let live = old.make(42);
    let dormant = old.callback(&live);
    let snapshot = through_bytes(&old.export(&live));
    let mut candidate = Fixture::source(V1);
    let target = candidate.schema(1);

    // Valid u64 data is not proof that a referenced player exists. Provenance is
    // host data too; it does not authorize the payload's business ID.
    let invalid_id = snapshot
        .migrate(
            &schema_version(1),
            &target,
            &SnapshotOptions::default(),
            |cx, value| {
                let StateValue::Record { key, mut fields } = value else {
                    panic!("migration expects the player DTO");
                };
                for (name, value) in &mut fields {
                    cx.charge_work(1)?;
                    if name == "player_id" {
                        *value = StateValue::U64(404);
                    }
                }
                Ok(StateValue::Record { key, fields })
            },
        )
        .unwrap();
    let invalid_id = through_bytes(&invalid_id);
    assert_eq!(invalid_id.header(), &header());
    let dto = candidate.restore_dto(&invalid_id, 1).unwrap();
    let rejected = candidate.restore_live(dto);
    assert_eq!(
        candidate.validate(&rejected),
        Err("unknown player ID".into())
    );
    drop(rejected);
    drop(candidate);

    assert_eq!(old.validate(&live), Ok(()));
    assert_eq!(old.balance(&live, "earn"), 24);
    assert_eq!(old.runtime.call(&dormant, ()).unwrap(), 700);
    assert_eq!(old.integer("next"), 6);
}
