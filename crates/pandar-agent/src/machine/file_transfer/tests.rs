use super::*;

#[test]
fn constants_match_runtime_policy() {
    assert_eq!(BAMBU_FILE_TRANSFER_PORT, 990);
    assert_eq!(BAMBU_FILE_TRANSFER_USERNAME, "bblp");
    assert_eq!(BAMBU_FILE_TRANSFER_CHUNK_SIZE, 64 * 1024);
}
