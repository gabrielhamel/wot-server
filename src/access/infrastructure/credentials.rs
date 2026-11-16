use crate::access::mercury::app::login::element::LoginError;
use serde::Deserialize;

#[derive(Deserialize)]
struct LoginEnvelope {
    login: String,
    auth_method: String,
}

pub fn account_username(wire_username: &str) -> Result<String, LoginError> {
    if !wire_username.trim_start().starts_with('{') {
        return Ok(wire_username.to_owned());
    }
    if wire_username.len() > 16 * 1024 {
        return Err(LoginError::MalformedRequest);
    }
    let envelope: LoginEnvelope =
        serde_json::from_str(wire_username).map_err(|_| LoginError::MalformedRequest)?;
    if envelope.auth_method != "basic" {
        return Err(LoginError::LoginNotAllowed);
    }
    Ok(envelope.login)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_basic_identity_and_rejects_official_tokens_and_malformed_envelopes() {
        assert_eq!(
            account_username(
                r#"{"login":"demo","auth_method":"basic","session":"abc","temporary":"1"}"#
            )
            .unwrap(),
            "demo"
        );
        assert_eq!(account_username("demo").unwrap(), "demo");
        assert_eq!(
            account_username(r#"{"login":"demo","auth_method":"token2","token2":"opaque"}"#),
            Err(LoginError::LoginNotAllowed)
        );
        assert_eq!(
            account_username(r#"{"login":"demo"}"#),
            Err(LoginError::MalformedRequest)
        );
        assert_eq!(
            account_username("{invalid"),
            Err(LoginError::MalformedRequest)
        );
    }
}
