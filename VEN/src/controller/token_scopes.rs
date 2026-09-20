//! What the VTN actually granted this VEN, read from its own token.
//!
//! GB-49: a VEN whose user exists but carries no VEN scopes authenticates
//! perfectly and then polls an **empty world** — every request succeeds, every
//! list comes back empty, and nothing anywhere says why. A VEN that sees no
//! events because there are none and a VEN that sees no events because it was
//! never granted `read_ven_objects` look identical from the outside, and the
//! second is a misprovisioning that can sit unnoticed for as long as nobody
//! expects an event.
//!
//! OpenADR 3.1 replaced 3.0's roles with scopes carried on the user, and the
//! access token states them, so the VEN can check its own grant instead of
//! inferring it from silence. This is defence in depth, not access control:
//! the VTN enforces scopes regardless. What it buys is that a misprovisioned
//! VEN says so.
//!
//! The token's signature is **not** verified here, deliberately. This code
//! decides what to tell the operator about our own grant, never whether to
//! trust a caller — the VTN is the only party that validates this token, and a
//! VEN lying to itself about its own scopes gains nothing.

use base64::Engine;

/// The scopes this lab provisions for a VEN (`VEN_SCOPES` in
/// `scripts/seed_vtn.py`). The two lists are one contract; they change
/// together or a VEN starts warning about a scope nobody intended it to have.
pub const EXPECTED_VEN_SCOPES: [&str; 3] =
    ["read_targets", "read_ven_objects", "write_reports_ven"];

/// The scopes named in a JWT's `scope` claim, in the order the token lists
/// them. Empty when the token is not a JWT, cannot be decoded, or names none —
/// all of which are reported the same way, because the VEN's question is "was
/// I granted what I need", and "cannot tell" is not "yes".
pub fn scopes_in_token(access_token: &str) -> Vec<String> {
    let Some(payload) = access_token.split('.').nth(1) else {
        return Vec::new();
    };
    let Ok(bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload) else {
        return Vec::new();
    };
    let Ok(claims) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Vec::new();
    };
    match claims.get("scope") {
        // RFC 8693 spells `scope` as one space-delimited string.
        Some(serde_json::Value::String(s)) => s.split_whitespace().map(str::to_string).collect(),
        // Some issuers use an array instead; accept both rather than reject a
        // conformant peer for a spelling the spec leaves open.
        Some(serde_json::Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

/// Which of the scopes this VEN needs are missing from its own token.
///
/// Empty means correctly provisioned. A non-empty result is worth surfacing
/// even though every request will still "succeed" — that is precisely the
/// failure mode.
pub fn missing_ven_scopes(access_token: &str) -> Vec<&'static str> {
    let granted = scopes_in_token(access_token);
    EXPECTED_VEN_SCOPES
        .iter()
        .copied()
        .filter(|needed| !granted.iter().any(|g| g == needed))
        .collect()
}

/// How a missing-scope finding reads on `/health` and in a notification.
pub fn describe_missing(missing: &[&'static str]) -> String {
    format!(
        "VTN token is missing {} VEN scope(s): {}. Requests will succeed and \
         return nothing until the VEN's user is granted them.",
        missing.len(),
        missing.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    /// Build an unsigned JWT-shaped token with the given claims body.
    fn token(claims: serde_json::Value) -> String {
        let b64 = |v: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
        format!(
            "{}.{}.{}",
            b64(br#"{"alg":"HS256","typ":"JWT"}"#),
            b64(claims.to_string().as_bytes()),
            "not-a-real-signature"
        )
    }

    #[test]
    fn scopes_in_token_reads_the_space_delimited_form() {
        let t = token(serde_json::json!({
            "scope": "read_targets read_ven_objects write_reports_ven"
        }));
        assert_eq!(
            scopes_in_token(&t),
            vec!["read_targets", "read_ven_objects", "write_reports_ven"]
        );
    }

    #[test]
    fn scopes_in_token_also_reads_an_array() {
        let t = token(serde_json::json!({"scope": ["read_targets", "write_reports_ven"]}));
        assert_eq!(
            scopes_in_token(&t),
            vec!["read_targets", "write_reports_ven"]
        );
    }

    /// The GB-49 case: the token is valid, the VEN authenticates, and it has
    /// been granted nothing. Every poll will succeed and return an empty world.
    #[test]
    fn missing_ven_scopes_names_every_scope_an_unscoped_token_lacks() {
        let t = token(serde_json::json!({"sub": "ven-1", "scope": ""}));
        assert_eq!(missing_ven_scopes(&t), EXPECTED_VEN_SCOPES.to_vec());
    }

    #[test]
    fn missing_ven_scopes_is_empty_for_a_correctly_provisioned_ven() {
        let t = token(serde_json::json!({
            "scope": "read_targets read_ven_objects write_reports_ven"
        }));
        assert!(missing_ven_scopes(&t).is_empty());
    }

    #[test]
    fn missing_ven_scopes_reports_only_what_is_actually_absent() {
        let t = token(serde_json::json!({"scope": "read_targets write_reports_ven"}));
        assert_eq!(missing_ven_scopes(&t), vec!["read_ven_objects"]);
    }

    /// An opaque (non-JWT) token cannot be read, and "cannot tell" is reported
    /// as "not granted" rather than assumed fine -- the whole point is to stop
    /// silence reading as success.
    #[test]
    fn an_opaque_token_reports_every_scope_as_missing() {
        assert!(scopes_in_token("opaque-token-value").is_empty());
        assert_eq!(
            missing_ven_scopes("opaque-token-value"),
            EXPECTED_VEN_SCOPES.to_vec()
        );
    }

    #[test]
    fn describe_missing_names_the_scopes_and_the_symptom() {
        let msg = describe_missing(&["read_ven_objects"]);
        assert!(msg.contains("read_ven_objects"), "got {msg}");
        assert!(msg.contains("return nothing"), "got {msg}");
    }
}
