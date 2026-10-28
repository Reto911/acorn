//! One configuration-driven cookie strategy for parsing, renewal and deletion.

use super::sessions::IDLE_TIMEOUT;
use axum::http::{HeaderMap, HeaderValue, header};

/// Cookie attributes derived from the browser-facing deployment protocol.
#[derive(Clone)]
pub(crate) struct CookiePolicy {
    /// Explicit startup setting, independent of the backend transport.
    secure: bool,
}

impl CookiePolicy {
    /// Select one consistent cookie name and attribute set for the process.
    pub(crate) fn new(secure: bool) -> Self {
        Self { secure }
    }

    /// Use the host-only browser prefix only when Secure is enabled.
    fn name(&self) -> &'static str {
        if self.secure {
            "__Host-acorn_session"
        } else {
            "acorn_session"
        }
    }

    /// Read exactly one correctly shaped token; ambiguous or malformed cookies are anonymous.
    pub(crate) fn read(&self, headers: &HeaderMap) -> Option<String> {
        let mut token = None;
        for header in headers.get_all(header::COOKIE) {
            for pair in header.to_str().ok()?.split(';') {
                let Some((name, value)) = pair.trim().split_once('=') else {
                    continue;
                };
                if name != self.name() {
                    continue;
                }
                if token.is_some()
                    || value.len() != 43
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                {
                    return None;
                }
                token = Some(value.to_owned());
            }
        }
        token
    }

    /// Set or renew a canonical token, or clear it with matching attributes and a zero age.
    pub(crate) fn header(&self, token: Option<&str>) -> HeaderValue {
        let age = if token.is_some() {
            IDLE_TIMEOUT.as_secs()
        } else {
            0
        };
        let secure = if self.secure { "; Secure" } else { "" };
        let mut value = HeaderValue::from_str(&format!(
            "{}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={age}{secure}",
            self.name(),
            token.unwrap_or_default()
        ))
        .expect("Generated base64url session token is a valid header");
        value.set_sensitive(true);
        value
    }
}

#[cfg(test)]
mod tests {
    //! Cookie lifetime and deployment variants share the same parsing and removal contract.
    use super::*;

    /// Both cookie names preserve their attributes on renewal and deletion.
    #[test]
    fn cookie_variants_and_ambiguous_input() {
        let token = "a".repeat(43);
        for secure in [false, true] {
            let policy = CookiePolicy::new(secure);
            let mut headers = HeaderMap::new();
            headers.insert(
                header::COOKIE,
                format!("other=x; {}={token}", policy.name())
                    .parse()
                    .unwrap(),
            );
            assert_eq!(policy.read(&headers), Some(token.clone()));
            let set = policy.header(Some(&token));
            assert!(set.is_sensitive());
            assert_eq!(
                set.to_str().unwrap(),
                format!(
                    "{}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=86400{}",
                    policy.name(),
                    if secure { "; Secure" } else { "" }
                )
            );
            assert_eq!(
                policy.header(None).to_str().unwrap(),
                format!(
                    "{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}",
                    policy.name(),
                    if secure { "; Secure" } else { "" }
                )
            );
            headers.append(
                header::COOKIE,
                format!("{}={token}", policy.name()).parse().unwrap(),
            );
            assert!(policy.read(&headers).is_none());
            headers.insert(
                header::COOKIE,
                format!("{}=bad", policy.name()).parse().unwrap(),
            );
            assert!(policy.read(&headers).is_none());
        }
    }
}
