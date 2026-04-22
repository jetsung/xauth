use anyhow::Result;
use data_encoding::BASE64URL_NOPAD;
use sha2::{Digest, Sha256};

/// 解码 state 数据：base64(JSON({redirect, state}))
pub fn decode_state(encoded: &str) -> Result<StateData> {
    let bytes = BASE64URL_NOPAD.decode(encoded.as_bytes())?;
    let state_data: StateData = serde_json::from_slice(&bytes)?;
    Ok(state_data)
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StateData {
    pub redirect: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callback_url: Option<String>,
}

/// PKCE code_verifier 生成
pub fn generate_pkce_verifier() -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes);
    BASE64URL_NOPAD.encode(&bytes)
}

/// PKCE code_challenge 生成 (S256)
pub fn generate_pkce_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let digest = hasher.finalize();
    BASE64URL_NOPAD.encode(&digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_roundtrip() {
        let state_obj = serde_json::json!({
            "redirect": "https://redirect.com",
            "state": "my-state"
        });
        let json_str = serde_json::to_string(&state_obj).unwrap();
        let encoded = BASE64URL_NOPAD.encode(json_str.as_bytes());
        let decoded = decode_state(&encoded).unwrap();
        assert_eq!(decoded.redirect, "https://redirect.com");
        assert_eq!(decoded.state, "my-state");
    }

    #[test]
    fn test_pkce() {
        let verifier = generate_pkce_verifier();
        let challenge = generate_pkce_challenge(&verifier);
        assert!(!challenge.is_empty());
        assert!(challenge.len() == 43);
    }
}
