//! Resolve runtime-provided process symbols against ABI-owned helper contracts.
use kagari_abi::native::{NativeHelperDeclaration, NativeHelperSymbol, NativeLinkDescription};
use kagari_abi::native_call::NATIVE_HELPER_SIGNATURES;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("native helper bindings must contain exactly one non-null address for each ABI helper")]
pub struct NativeLinkError;

/// Describes ABI signatures, not proof that arbitrary addresses are callable.
/// SDK orchestration obtains these bindings from runtime's own symbol provider.
pub fn build_native_links(
    symbols: &[NativeHelperSymbol],
) -> Result<NativeLinkDescription, NativeLinkError> {
    if symbols.len() != NATIVE_HELPER_SIGNATURES.len() {
        return Err(NativeLinkError);
    }
    let helpers = NATIVE_HELPER_SIGNATURES
        .iter()
        .map(|signature| {
            let mut matches = symbols
                .iter()
                .filter(|symbol| symbol.symbol == signature.symbol);
            let symbol = matches.next().ok_or(NativeLinkError)?;
            if symbol.address == 0 || matches.next().is_some() {
                return Err(NativeLinkError);
            }
            Ok(NativeHelperDeclaration {
                symbol: signature.symbol.into(),
                address: symbol.address,
                parameters: signature.parameters.to_vec(),
                results: signature.results.to_vec(),
            })
        })
        .collect::<Result<_, _>>()?;
    Ok(NativeLinkDescription { helpers })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::slice;

    #[test]
    fn resolves_only_complete_unique_non_null_helper_sets() {
        let symbol = NativeHelperSymbol {
            symbol: NATIVE_HELPER_SIGNATURES[0].symbol.into(),
            address: 42,
        };
        let links = build_native_links(slice::from_ref(&symbol)).unwrap();
        assert_eq!(
            links.helpers[0].parameters,
            NATIVE_HELPER_SIGNATURES[0].parameters
        );
        assert_eq!(
            links.helpers[0].results,
            NATIVE_HELPER_SIGNATURES[0].results
        );
        assert_eq!(links.helpers[0].address, 42);
        assert!(build_native_links(&[]).is_err());
        assert!(build_native_links(&[symbol.clone(), symbol.clone()]).is_err());
        assert!(
            build_native_links(&[NativeHelperSymbol {
                address: 0,
                ..symbol.clone()
            }])
            .is_err()
        );
        assert!(
            build_native_links(&[NativeHelperSymbol {
                symbol: "unknown".into(),
                ..symbol
            }])
            .is_err()
        );
    }
}
