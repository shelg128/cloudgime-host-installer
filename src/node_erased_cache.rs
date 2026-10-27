//! Only erased non-secret caches may bootstrap again from canonical Node identity.
//! A valid cache with a different owner and arbitrary malformed JSON stay fail-closed.
use serde::de::DeserializeOwned;

pub const MAX_CACHE_BYTES: usize = 65_536;
pub struct ParsedCache<T> {
    pub state: T,
    pub erased: bool,
}
pub fn parse<T: DeserializeOwned + Default>(raw: &[u8]) -> Result<ParsedCache<T>, &'static str> {
    if raw.len() > MAX_CACHE_BYTES {
        return Err("NODE_CACHE_TOO_LARGE");
    }
    let bytes = raw.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(raw);
    let erased = bytes.is_empty()
        || bytes.iter().all(|b| *b == 0)
        || bytes.iter().all(u8::is_ascii_whitespace);
    if erased {
        return Ok(ParsedCache {
            state: T::default(),
            erased: true,
        });
    }
    let state = serde_json::from_slice(bytes).map_err(|_| "NODE_CACHE_INVALID_JSON")?;
    Ok(ParsedCache {
        state,
        erased: false,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    #[derive(Debug, Default, Deserialize)]
    #[serde(default)]
    struct State {
        #[serde(rename = "HostId")]
        host: String,
        #[serde(rename = "ActivationState")]
        phase: String,
    }
    #[test]
    fn recorded_2172_zero_bytes_require_fresh_canonical_revalidation() {
        let p = parse::<State>(&vec![0; 2172]).unwrap();
        assert!(p.erased);
        assert!(p.state.host.is_empty());
        assert!(p.state.phase.is_empty());
    }
    #[test]
    fn missing_empty_whitespace_and_bom_are_erased() {
        for b in [vec![], b" \r\n\t".to_vec(), vec![0xef, 0xbb, 0xbf]] {
            assert!(parse::<State>(&b).unwrap().erased);
        }
    }
    #[test]
    fn valid_other_binding_is_preserved_for_conflict_check() {
        let p = parse::<State>(br#"{"HostId":"other-owner","ActivationState":"revoked"}"#).unwrap();
        assert!(!p.erased);
        assert_eq!(p.state.host, "other-owner");
        assert_eq!(p.state.phase, "revoked");
    }
    #[test]
    fn arbitrary_corruption_never_discards_existing_identity() {
        for raw in [
            &b"{\"HostId\":\"other"[..],
            &b"bad"[..],
            &b"{}\0"[..],
            &b"\0 \0"[..],
            &b"null"[..],
        ] {
            assert!(parse::<State>(raw).is_err());
        }
    }
    #[test]
    fn oversized_cache_is_not_read_as_default() {
        assert_eq!(
            parse::<State>(&vec![0; MAX_CACHE_BYTES + 1]).err(),
            Some("NODE_CACHE_TOO_LARGE")
        );
    }
    #[test]
    fn ordinary_bom_json_remains_a_normal_cache() {
        let p = parse::<State>(b"\xef\xbb\xbf{\"HostId\":\"same\"}").unwrap();
        assert!(!p.erased);
        assert_eq!(p.state.host, "same");
    }
}
