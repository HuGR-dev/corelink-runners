//! Public verifier configuration. Missing configuration never enables a grant.
use base64::Engine as _;
use serde::de::{MapAccess, Visitor};
use std::collections::HashMap;

pub(crate) fn parse(raw: Option<&str>) -> anyhow::Result<HashMap<String, Vec<u8>>> {
    let Some(raw) = raw else {
        return Ok(HashMap::new());
    };
    anyhow::ensure!(
        raw.len() <= 8192,
        "compute public key configuration too large"
    );
    struct Keys;
    impl<'de> Visitor<'de> for Keys {
        type Value = HashMap<String, Vec<u8>>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("unique compute issuer public keys")
        }
        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
            let mut keys = HashMap::new();
            while let Some((id, encoded)) = map.next_entry::<String, String>()? {
                if id.is_empty()
                    || id.len() > 64
                    || keys.len() >= 32
                    || keys.contains_key(&id)
                    || !id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
                {
                    return Err(serde::de::Error::custom("invalid compute key identity"));
                }
                let key = base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .map_err(|_| serde::de::Error::custom("invalid compute public key"))?;
                if key.len() != 32 {
                    return Err(serde::de::Error::custom(
                        "invalid compute public key length",
                    ));
                }
                keys.insert(id, key);
            }
            Ok(keys)
        }
    }
    let mut decoder = serde_json::Deserializer::from_str(raw);
    let keys = serde::Deserializer::deserialize_map(&mut decoder, Keys)
        .map_err(|_| anyhow::anyhow!("invalid compute public key configuration"))?;
    decoder
        .end()
        .map_err(|_| anyhow::anyhow!("invalid compute public key configuration"))?;
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_keys_are_unarmed_and_rotation_preserves_both() {
        assert!(parse(None).unwrap().is_empty());
        let key = base64::engine::general_purpose::STANDARD.encode([7u8; 32]);
        let keys = parse(Some(&format!(r#"{{"old":"{key}","new":"{key}"}}"#))).unwrap();
        assert_eq!(keys.len(), 2);
    }
    #[test]
    fn invalid_or_duplicate_configuration_refuses_without_echoing_material() {
        for raw in [
            "",
            "null",
            "[]",
            r#"{"key":"secret-invalid-value"}"#,
            r#"{"key":"","key":""}"#,
            "{} trailing",
        ] {
            let err = parse(Some(raw)).unwrap_err().to_string();
            assert!(!err.contains("secret-invalid-value"));
        }
    }
}
