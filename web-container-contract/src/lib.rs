use byteorder::{BigEndian, ReadBytesExt};
use ciborium::{de::from_reader, ser::into_writer};
use ed25519_dalek::{Signature, VerifyingKey};
use freenet_stdlib::prelude::*;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read};

const MAX_METADATA_SIZE: u64 = 1024; // 1KB
const MAX_WEB_SIZE: u64 = 1024 * 1024 * 100; // 100MB

/// Ed25519-signed metadata for web container state.
#[derive(Serialize, Deserialize)]
pub struct WebContainerMetadata {
    pub version: u32,
    pub signature: Signature,
}

pub struct WebContainerContract;

#[contract]
impl ContractInterface for WebContainerContract {
    fn validate_state(
        parameters: Parameters<'static>,
        state: State<'static>,
        _related: RelatedContracts<'static>,
    ) -> Result<ValidateResult, ContractError> {
        // Extract verifying key from first 32 bytes of parameters.
        // Additional bytes (e.g. a vanity nonce) are ignored.
        let params_bytes: &[u8] = parameters.as_ref();
        if params_bytes.len() < 32 {
            return Err(ContractError::Other(
                "Parameters must be at least 32 bytes (Ed25519 public key)".to_string(),
            ));
        }
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&params_bytes[..32]);

        let verifying_key = VerifyingKey::from_bytes(&key_bytes)
            .map_err(|e| ContractError::Other(format!("Invalid public key: {}", e)))?;

        // Parse state: [metadata_length: u64][metadata: bytes][web_length: u64][web: bytes]
        let mut cursor = Cursor::new(state.as_ref());

        let metadata_size = cursor
            .read_u64::<BigEndian>()
            .map_err(|e| ContractError::Other(format!("Failed to read metadata size: {}", e)))?;

        if metadata_size > MAX_METADATA_SIZE {
            return Err(ContractError::Other(format!(
                "Metadata size {} exceeds maximum allowed size of {} bytes",
                metadata_size, MAX_METADATA_SIZE
            )));
        }

        let mut metadata_bytes = vec![0; metadata_size as usize];
        cursor
            .read_exact(&mut metadata_bytes)
            .map_err(|e| ContractError::Other(format!("Failed to read metadata: {}", e)))?;

        let metadata: WebContainerMetadata =
            from_reader(&metadata_bytes[..]).map_err(|e| ContractError::Deser(e.to_string()))?;

        if metadata.version == 0 {
            return Err(ContractError::InvalidState);
        }

        let web_size = cursor
            .read_u64::<BigEndian>()
            .map_err(|e| ContractError::Other(format!("Failed to read web size: {}", e)))?;

        if web_size > MAX_WEB_SIZE {
            return Err(ContractError::Other(format!(
                "Web size {} exceeds maximum allowed size of {} bytes",
                web_size, MAX_WEB_SIZE
            )));
        }

        let mut webapp_bytes = vec![0; web_size as usize];
        cursor
            .read_exact(&mut webapp_bytes)
            .map_err(|e| ContractError::Other(format!("Failed to read web bytes: {}", e)))?;

        // Verify signature over (version || web content)
        let mut message = metadata.version.to_be_bytes().to_vec();
        message.extend_from_slice(&webapp_bytes);

        verifying_key
            .verify_strict(&message, &metadata.signature)
            .map_err(|e| ContractError::Other(format!("Signature verification failed: {}", e)))?;

        Ok(ValidateResult::Valid)
    }

    fn update_state(
        _parameters: Parameters<'static>,
        state: State<'static>,
        data: Vec<UpdateData<'static>>,
    ) -> Result<UpdateModification<'static>, ContractError> {
        let current_version = if state.as_ref().is_empty() {
            0
        } else {
            let mut cursor = Cursor::new(state.as_ref());
            let metadata_size = cursor.read_u64::<BigEndian>().map_err(|e| {
                ContractError::Other(format!("Failed to read metadata size: {}", e))
            })?;
            let mut metadata_bytes = vec![0; metadata_size as usize];
            cursor
                .read_exact(&mut metadata_bytes)
                .map_err(|e| ContractError::Other(format!("Failed to read metadata: {}", e)))?;
            let metadata: WebContainerMetadata = from_reader(&metadata_bytes[..])
                .map_err(|e| ContractError::Deser(e.to_string()))?;
            metadata.version
        };

        if let Some(UpdateData::State(new_state)) = data.into_iter().next() {
            let mut cursor = Cursor::new(new_state.as_ref());
            let metadata_size = cursor.read_u64::<BigEndian>().map_err(|e| {
                ContractError::Other(format!("Failed to read metadata size: {}", e))
            })?;
            let mut metadata_bytes = vec![0; metadata_size as usize];
            cursor
                .read_exact(&mut metadata_bytes)
                .map_err(|e| ContractError::Other(format!("Failed to read metadata: {}", e)))?;
            let metadata: WebContainerMetadata = from_reader(&metadata_bytes[..])
                .map_err(|e| ContractError::Deser(e.to_string()))?;

            if metadata.version <= current_version {
                return Err(ContractError::InvalidUpdateWithInfo {
                    reason: format!(
                        "New state version {} must be higher than current version {}",
                        metadata.version, current_version
                    ),
                });
            }

            Ok(UpdateModification::valid(new_state))
        } else {
            Err(ContractError::InvalidUpdate)
        }
    }

    fn summarize_state(
        _parameters: Parameters<'static>,
        state: State<'static>,
    ) -> Result<StateSummary<'static>, ContractError> {
        if state.as_ref().is_empty() {
            return Ok(StateSummary::from(Vec::new()));
        }

        let mut cursor = Cursor::new(state.as_ref());
        let metadata_size = cursor
            .read_u64::<BigEndian>()
            .map_err(|e| ContractError::Other(format!("Failed to read metadata size: {}", e)))?;
        let mut metadata_bytes = vec![0; metadata_size as usize];
        cursor
            .read_exact(&mut metadata_bytes)
            .map_err(|e| ContractError::Other(format!("Failed to read metadata: {}", e)))?;
        let metadata: WebContainerMetadata =
            from_reader(&metadata_bytes[..]).map_err(|e| ContractError::Deser(e.to_string()))?;

        let mut summary = Vec::new();
        into_writer(&metadata.version, &mut summary)
            .map_err(|e| ContractError::Deser(e.to_string()))?;

        Ok(StateSummary::from(summary))
    }

    fn get_state_delta(
        _parameters: Parameters<'static>,
        state: State<'static>,
        summary: StateSummary<'static>,
    ) -> Result<StateDelta<'static>, ContractError> {
        if state.as_ref().is_empty() {
            return Ok(StateDelta::from(Vec::new()));
        }

        let current_version = {
            let mut cursor = Cursor::new(state.as_ref());
            let metadata_size = cursor.read_u64::<BigEndian>().map_err(|e| {
                ContractError::Other(format!("Failed to read metadata size: {}", e))
            })?;
            let mut metadata_bytes = vec![0; metadata_size as usize];
            cursor
                .read_exact(&mut metadata_bytes)
                .map_err(|e| ContractError::Other(format!("Failed to read metadata: {}", e)))?;
            let metadata: WebContainerMetadata = from_reader(&metadata_bytes[..])
                .map_err(|e| ContractError::Deser(e.to_string()))?;
            metadata.version
        };

        let summary_version: u32 =
            from_reader(summary.as_ref()).map_err(|e| ContractError::Deser(e.to_string()))?;

        if current_version > summary_version {
            Ok(StateDelta::from(state.as_ref().to_vec()))
        } else {
            Ok(StateDelta::from(Vec::new()))
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    fn create_test_keypair() -> (SigningKey, VerifyingKey) {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        (signing_key, verifying_key)
    }

    fn create_test_state(version: u32, web: &[u8], signing_key: &SigningKey) -> Vec<u8> {
        let mut message = version.to_be_bytes().to_vec();
        message.extend_from_slice(web);
        let signature = signing_key.sign(&message);
        let metadata = WebContainerMetadata { version, signature };
        let mut metadata_bytes = Vec::new();
        into_writer(&metadata, &mut metadata_bytes).unwrap();

        let mut state = Vec::new();
        state.extend_from_slice(&(metadata_bytes.len() as u64).to_be_bytes());
        state.extend_from_slice(&metadata_bytes);
        state.extend_from_slice(&(web.len() as u64).to_be_bytes());
        state.extend_from_slice(web);
        state
    }

    #[test]
    fn test_valid_state_32_byte_params() {
        let (signing_key, verifying_key) = create_test_keypair();
        let state = create_test_state(1, b"Hello", &signing_key);
        let result = WebContainerContract::validate_state(
            Parameters::from(verifying_key.to_bytes().to_vec()),
            State::from(state),
            RelatedContracts::default(),
        );
        assert!(matches!(result, Ok(ValidateResult::Valid)));
    }

    #[test]
    fn test_valid_state_with_nonce() {
        let (signing_key, verifying_key) = create_test_keypair();
        let state = create_test_state(1, b"Hello", &signing_key);

        // 32-byte key + 8-byte vanity nonce
        let mut params = verifying_key.to_bytes().to_vec();
        params.extend_from_slice(&42u64.to_le_bytes());

        let result = WebContainerContract::validate_state(
            Parameters::from(params),
            State::from(state),
            RelatedContracts::default(),
        );
        assert!(matches!(result, Ok(ValidateResult::Valid)));
    }

    #[test]
    fn test_params_too_short() {
        let result = WebContainerContract::validate_state(
            Parameters::from(vec![0u8; 16]),
            State::from(vec![]),
            RelatedContracts::default(),
        );
        assert!(matches!(result, Err(ContractError::Other(_))));
    }

    #[test]
    fn test_invalid_signature() {
        let (_, verifying_key) = create_test_keypair();
        let (wrong_key, _) = create_test_keypair();
        let state = create_test_state(1, b"Hello", &wrong_key);
        let result = WebContainerContract::validate_state(
            Parameters::from(verifying_key.to_bytes().to_vec()),
            State::from(state),
            RelatedContracts::default(),
        );
        assert!(matches!(result, Err(ContractError::Other(_))));
    }

    #[test]
    fn test_version_must_increase() {
        let (signing_key, _) = create_test_keypair();
        let current = create_test_state(2, b"Old", &signing_key);
        let update = create_test_state(2, b"New", &signing_key);
        let result = WebContainerContract::update_state(
            Parameters::from(vec![]),
            State::from(current),
            vec![UpdateData::State(State::from(update))],
        );
        assert!(matches!(
            result,
            Err(ContractError::InvalidUpdateWithInfo { .. })
        ));
    }
}
