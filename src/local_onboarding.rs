//! Narrow local provisioning authentication. It supplements rental middleware
//! only during a bound first boot; ordinary Host session/admin checks still run.
use actix_web::HttpRequest;
use serde_json::Value;
use std::path::PathBuf;

pub(crate) fn route_allowed(method: &str, path: &str) -> bool {
    match method {
        "POST" => matches!(path, "/login" | "/host" | "/pair"),
        "GET" => {
            matches!(path, "/host" | "/apps")
                || path.strip_prefix("/host/").is_some_and(|tail| {
                    let mut p = tail.split('/');
                    p.next()
                        .is_some_and(|id| !id.is_empty() && id.bytes().all(|c| c.is_ascii_digit()))
                        && matches!(p.next(), None | Some("apps" | "pair"))
                        && p.next().is_none()
                })
        }
        _ => false,
    }
}
pub(crate) fn authenticated(req: &HttpRequest, route: &str) -> bool {
    if !route_allowed(req.method().as_str(), route)
        || !req.peer_addr().is_some_and(|p| p.ip().is_loopback())
        || req.headers().contains_key("forwarded")
        || req.headers().contains_key("x-forwarded-for")
        || req.headers().contains_key("origin")
        || req.headers().get_all("x-cloudgime-onboarding").count() != 1
    {
        return false;
    }
    let Some(token) = req
        .headers()
        .get("x-cloudgime-onboarding")
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    if token.len() < 16 || token.len() > 2048 || token.chars().any(char::is_whitespace) {
        return false;
    }
    verify(token).unwrap_or(false)
}
#[cfg(windows)]
fn verify(token: &str) -> anyhow::Result<bool> {
    let root =
        PathBuf::from(std::env::var_os("ProgramData").ok_or_else(|| anyhow::anyhow!("ROOT"))?)
            .join("CloudGime")
            .join("provisioning");
    let handoff: Value = serde_json::from_slice(&crate::provisioning::read_onboarding_private(
        &root.join("first-boot-shell-private/handoff.json"),
    )?)?;
    if !matches!(handoff["Stage"].as_str(), Some("ARMED" | "CONFIRMED")) {
        return Ok(false);
    }
    let operational = root.join("first-boot-shell-private/operational/operational.json");
    if operational.exists() {
        let state: Value =
            serde_json::from_slice(&crate::provisioning::read_onboarding_private(&operational)?)?;
        if state["Stage"] == "OPERATIONAL" {
            return Ok(false);
        }
    }
    let path = root.join("host_app.credential.dpapi");
    let _cipher = crate::provisioning::read_onboarding_private(&path)?;
    let mut bytes = crate::provisioning::read_dpapi_secret(&path)?;
    let parsed = serde_json::from_slice::<Value>(&bytes);
    bytes.fill(0);
    let value = parsed?;
    let expected = value["deviceToken"].as_str().unwrap_or("");
    let manifest: Value = serde_json::from_slice(&std::fs::read(root.join("manifest.json"))?)?;
    if manifest.pointer("/binding/pcId").and_then(Value::as_str)
        != handoff.pointer("/Binding/PcId").and_then(Value::as_str)
    {
        return Ok(false);
    }
    Ok(!expected.is_empty()
        && expected.len() == token.len()
        && openssl::memcmp::eq(expected.as_bytes(), token.as_bytes())
        && value["componentType"].as_str() == Some("host_app"))
}
#[cfg(not(windows))]
fn verify(_: &str) -> anyhow::Result<bool> {
    Ok(false)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_network_and_browser_requests() {
        use actix_web::test::TestRequest;
        for req in [
            TestRequest::post()
                .peer_addr("192.0.2.1:1234".parse().unwrap())
                .insert_header(("x-cloudgime-onboarding", "fixture-only-token-value"))
                .to_http_request(),
            TestRequest::post()
                .peer_addr("127.0.0.1:1234".parse().unwrap())
                .insert_header(("origin", "https://cloudgime.my.id"))
                .insert_header(("x-cloudgime-onboarding", "fixture-only-token-value"))
                .to_http_request(),
            TestRequest::post()
                .peer_addr("127.0.0.1:1234".parse().unwrap())
                .insert_header(("x-forwarded-for", "127.0.0.1"))
                .insert_header(("x-cloudgime-onboarding", "fixture-only-token-value"))
                .to_http_request(),
            TestRequest::post()
                .peer_addr("127.0.0.1:1234".parse().unwrap())
                .append_header(("x-cloudgime-onboarding", "fixture-only-token-value"))
                .append_header(("x-cloudgime-onboarding", "fixture-only-token-value"))
                .to_http_request(),
        ] {
            assert!(!authenticated(&req, "/login"));
        }
    }
    #[test]
    fn permits_pairing_only() {
        assert!(route_allowed("POST", "/login"));
        assert!(route_allowed("POST", "/pair"));
        assert!(route_allowed("GET", "/host/1/apps"));
        for (m, p) in [
            ("POST", "/host/stream"),
            ("GET", "/host/stream"),
            ("POST", "/users"),
            ("DELETE", "/host/1"),
            ("POST", "/admin"),
            ("POST", "/android-native/launch-token"),
        ] {
            assert!(!route_allowed(m, p));
        }
    }
}
