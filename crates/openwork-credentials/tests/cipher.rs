use openwork_credentials::{ApiKeyCipher, ApiKeyCipherError};

#[test]
fn encrypted_credentials_round_trip_and_are_bound_to_the_provider() {
    let cipher = ApiKeyCipher::from_key([7; 32]);
    let first = cipher.encrypt("provider-a", "secret").unwrap();
    let second = cipher.encrypt("provider-a", "secret").unwrap();

    assert_ne!(first, second);
    assert_eq!(cipher.decrypt("provider-a", &first).unwrap(), "secret");
    assert_eq!(
        cipher.decrypt("provider-b", &first),
        Err(ApiKeyCipherError::DecryptionFailed)
    );
}

#[test]
fn cipher_debug_output_never_contains_key_material() {
    let cipher = ApiKeyCipher::from_key([11; 32]);
    assert_eq!(format!("{cipher:?}"), "ApiKeyCipher([REDACTED])");
}
