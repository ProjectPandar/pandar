use super::*;

#[test]
fn resolves_four_part_studio_versions_by_first_three_components() {
    assert_eq!(
        resolve_studio_version("02.07.01.62").unwrap().id,
        "02.07.01"
    );
    assert_eq!(resolve_studio_version("2.7.1.62").unwrap().id, "02.07.01");
    assert_eq!(
        resolve_studio_version("02.07.01.99").unwrap().id,
        "02.07.01"
    );
    assert_eq!(
        resolve_studio_version("02.08.01.55").unwrap().id,
        "02.08.01"
    );
    assert!(resolve_studio_version("02.09.00.00").is_err());
    assert!(resolve_studio_version("02.07.01").is_err());
}
