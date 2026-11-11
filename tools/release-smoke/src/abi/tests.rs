use std::collections::BTreeSet;

#[test]
fn native_symbol_parsers_ignore_undefined_target_prefix_symbols() {
    assert_eq!(
        super::parse_exported_symbols(
            super::SymbolOutput::Nm,
            "                 U bambu_network_missing\n0000 T _ft_abi_version"
        ),
        BTreeSet::from(["ft_abi_version".to_owned()])
    );
    assert_eq!(
        super::parse_exported_symbols(
            super::SymbolOutput::Readelf,
            "1: 0 0 FUNC GLOBAL DEFAULT UND bambu_network_missing\n2: 1 0 FUNC GLOBAL DEFAULT 12 bambu_network_get_version"
        ),
        BTreeSet::from(["bambu_network_get_version".to_owned()])
    );
}
