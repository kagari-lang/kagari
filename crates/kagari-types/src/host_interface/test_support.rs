//! Path-authored fixtures adopt once before checking the scoped Host contract.
use crate::host_interface::{
    HostFunctionDeclaration, HostInterface, HostInterfaceError, MAGIC, VERSION, adoption_error,
    path::{
        HostIndexSegmentDeclaration, HostPathContract, HostPathDeclaration,
        HostVirtualSegmentDeclaration,
    },
    type_declaration::{HostFieldDeclaration, HostMethodDeclaration, HostTypeDeclaration},
    value_type::HostValueType,
    wire,
};
use kagari_common::identity::{
    DefinitionPath,
    metadata::{DefinitionMetadata, scope_record},
    table::DefinitionId,
};

pub(super) trait ValidateFixture {
    fn validate_fixture(&self) -> Result<(), HostInterfaceError>;
}

pub(super) trait FingerprintFixture {
    fn fingerprint_fixture(&self) -> Result<u64, HostInterfaceError>;
}

macro_rules! scoped_validation {
    ($($record:ty),+ $(,)?) => {$ (
        impl ValidateFixture for $record {
            fn validate_fixture(&self) -> Result<(), HostInterfaceError> {
                let scope = scope_record(self, &Default::default()).map_err(adoption_error)?;
                scope.records().validate(scope.definitions())
            }
        }
    )+};
}

scoped_validation!(
    HostInterface,
    HostTypeDeclaration,
    HostFunctionDeclaration,
    HostIndexSegmentDeclaration,
    HostVirtualSegmentDeclaration
);

macro_rules! scoped_fingerprint {
    ($($record:ty),+ $(,)?) => {$ (
        impl FingerprintFixture for $record {
            fn fingerprint_fixture(&self) -> Result<u64, HostInterfaceError> {
                let scope = scope_record(self, &Default::default()).map_err(adoption_error)?;
                scope.records().fingerprint(scope.definitions())
            }
        }
    )+};
}

scoped_fingerprint!(
    HostTypeDeclaration,
    HostFunctionDeclaration,
    HostFieldDeclaration,
    HostMethodDeclaration,
    HostValueType,
    HostPathContract
);

pub(super) trait InterfaceFixture {
    fn encode_fixture(&self) -> Result<Vec<u8>, HostInterfaceError>;

    fn path_fixture(
        &self,
        path: &HostPathDeclaration,
    ) -> Result<HostPathContract, HostInterfaceError>;
}

impl InterfaceFixture for HostInterface {
    fn encode_fixture(&self) -> Result<Vec<u8>, HostInterfaceError> {
        let scope = scope_record(self, &Default::default()).map_err(adoption_error)?;
        scope.records().to_bytes(scope.definitions())
    }

    fn path_fixture(
        &self,
        path: &HostPathDeclaration,
    ) -> Result<HostPathContract, HostInterfaceError> {
        let mut input = self.clone();
        // The queried path may not be among the published declarations.
        if !input.paths.contains(path) {
            input.paths.push(path.clone());
        }
        let index = input
            .paths
            .iter()
            .position(|candidate| candidate == path)
            .unwrap();
        let scope = scope_record(&input, &Default::default()).map_err(adoption_error)?;
        let record = scope
            .records()
            .path_contract(&scope.records().paths[index], scope.definitions())?;
        DefinitionMetadata::checked(scope.definitions().clone(), record, &Default::default())
            .map_err(adoption_error)?
            .to_paths(&Default::default())
            .map_err(adoption_error)
    }
}

pub(super) trait PathFixture {
    fn contract_fixture(
        &self,
        interface: &HostInterface,
    ) -> Result<HostPathContract, HostInterfaceError>;
}

impl PathFixture for HostPathDeclaration {
    fn contract_fixture(
        &self,
        interface: &HostInterface,
    ) -> Result<HostPathContract, HostInterfaceError> {
        interface.path_fixture(self)
    }
}

pub(super) trait TypeFixture {
    fn method_fixture(
        &self,
        method: &DefinitionPath,
    ) -> Result<HostFunctionDeclaration, HostInterfaceError>;
}

impl TypeFixture for HostTypeDeclaration {
    fn method_fixture(
        &self,
        method: &DefinitionPath,
    ) -> Result<HostFunctionDeclaration, HostInterfaceError> {
        let scope = scope_record(self, &Default::default()).map_err(adoption_error)?;
        let id = scope
            .definitions()
            .lookup(method)
            .ok_or(HostInterfaceError::InvalidDeclaration)?;
        let record = scope.records().method_contract(&id, scope.definitions())?;
        DefinitionMetadata::checked(scope.definitions().clone(), record, &Default::default())
            .map_err(adoption_error)?
            .to_paths(&Default::default())
            .map_err(adoption_error)
    }
}

pub(super) fn decode_fixture(bytes: &[u8]) -> Result<HostInterface, HostInterfaceError> {
    HostInterface::<DefinitionId>::from_bytes(bytes)?
        .to_paths(&Default::default())
        .map_err(adoption_error)
}

impl ValidateFixture for HostValueType {
    fn validate_fixture(&self) -> Result<(), HostInterfaceError> {
        let scope = scope_record(self, &Default::default()).map_err(adoption_error)?;
        scope.records().validate_in(scope.definitions())
    }
}

pub(super) fn encode_unchecked_fixture(
    interface: &HostInterface,
) -> Result<Vec<u8>, HostInterfaceError> {
    let scope = scope_record(interface, &Default::default()).map_err(adoption_error)?;
    let mut bytes = MAGIC.to_vec();
    bytes.extend(VERSION.to_le_bytes());
    bytes.extend(wire::canonical_bytes(scope.records(), scope.definitions())?);
    Ok(bytes)
}
