use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

/// Build configuration, not a readiness receipt. Never turn old connected=true into proof.
pub fn configure_route(
    value: &Value,
    activation_id: &str,
    port: i32,
    prefix: &str,
) -> Result<Value> {
    if activation_id.is_empty()
        || activation_id.len() > 256
        || !activation_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        bail!("invalid transport activation");
    }
    if !(1..=65535).contains(&port) {
        bail!("invalid assigned Host HTTP port");
    }
    let prefix = prefix.trim_matches('/');
    if prefix.is_empty()
        || !prefix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        bail!("invalid local stream prefix");
    }
    let mut output = value.clone();
    let routes = output
        .get_mut("routes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("keeper routes missing"))?;
    let count = routes
        .iter()
        .filter(|r| r.get("name").and_then(Value::as_str) == Some("moonlight"))
        .count();
    if count > 1 {
        bail!("ambiguous moonlight route");
    }
    if count == 0 {
        routes.push(json!({"name":"moonlight"}));
    }
    let route = routes
        .iter_mut()
        .find(|r| r.get("name").and_then(Value::as_str) == Some("moonlight"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow!("invalid moonlight route"))?;
    if let Some(old) = route.get("applicationActivationId").and_then(Value::as_str) {
        if !old.is_empty() && old != activation_id {
            bail!("transport activation mismatch");
        }
    }
    let local_url = format!("http://127.0.0.1:{port}/{prefix}/");
    route.insert("applicationActivationId".into(), json!(activation_id));
    route.insert("applicationType".into(), json!("CLOUDGIME_HOST"));
    route.insert("routeKey".into(), json!("HOST_STREAM_PROXY_ROUTE"));
    route.insert("localUrl".into(), json!(local_url));
    route.insert("moonlightLocalUrl".into(), json!(local_url));
    route.insert("connected".into(), json!(false));
    // Old instance may refer to a different PC-number. The agent uses its backend-issued env binding.
    route.remove("instance");
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creates_empty_first_install() {
        let r = configure_route(&json!({"routes":[]}), "act-test", 18037, "/stream").unwrap();
        assert_eq!(r["routes"][0]["localUrl"], "http://127.0.0.1:18037/stream/");
        assert_eq!(r["routes"][0]["routeKey"], "HOST_STREAM_PROXY_ROUTE");
        assert_eq!(r["routes"][0]["connected"], false);
    }
    #[test]
    fn idempotent_preserves_other_routes() {
        let r = configure_route(
            &json!({"routes":[{"name":"panel","localUrl":"unchanged"}]}),
            "a",
            1234,
            "/custom/",
        )
        .unwrap();
        assert_eq!(r, configure_route(&r, "a", 1234, "/custom/").unwrap());
        assert_eq!(r["routes"][0]["localUrl"], "unchanged");
    }
    #[test]
    fn rejects_wrong_binding_or_ambiguous_route() {
        assert!(
            configure_route(
                &json!({"routes":[{"name":"moonlight","applicationActivationId":"old"}]}),
                "new",
                1234,
                "/s"
            )
            .is_err()
        );
        assert!(
            configure_route(
                &json!({"routes":[{"name":"moonlight"},{"name":"moonlight"}]}),
                "a",
                1234,
                "/s"
            )
            .is_err()
        );
        assert!(configure_route(&json!({}), "a", 1234, "/s").is_err());
    }
    #[test]
    fn rejects_unsafe_inputs() {
        for port in [0, -1, 65536] {
            assert!(configure_route(&json!({"routes":[]}), "a", port, "s").is_err());
        }
        for prefix in ["", "../stream", "s?x=1", "s%2f", "a/b", "s#fragment"] {
            assert!(configure_route(&json!({"routes":[]}), "a", 1234, prefix).is_err());
        }
        for activation in ["", "a\nb", "a=b"] {
            assert!(configure_route(&json!({"routes":[]}), activation, 1234, "s").is_err());
        }
    }
    #[test]
    fn clears_stale_ready_and_instance() {
        let r = configure_route(
            &json!({"routes":[{"name":"moonlight","connected":true,"instance":1}]}),
            "a",
            2345,
            "s",
        )
        .unwrap();
        assert_eq!(r["routes"][0]["connected"], false);
        assert!(r["routes"][0].get("instance").is_none());
    }
}
