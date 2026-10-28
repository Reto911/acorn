//! Shared account input rules and blocking Argon2id password operations.

pub(crate) mod cookie;
pub(crate) mod sessions;

use argon2::{
    Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version,
    password_hash::{self, SaltString},
};

/// Public account identity, separate from stored password credentials.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    /// Server-assigned signed MySQL identity.
    pub id: i64,
    /// Normalized lowercase login name.
    pub username: String,
    /// Plain display name with no permission meaning.
    pub display_name: String,
}

/// Immutable identity accepted at the HTTP boundary, independent of later session changes.
#[derive(Clone)]
pub struct AuthenticatedUser {
    /// Account resolved from a valid server-side session.
    pub user: User,
}

/// Validated registration data; password contents are never formatted or serialized.
pub struct Registration {
    /// Normalized lowercase login name.
    pub username: String,
    /// Validated display name, including its resolved default.
    pub display_name: String,
    /// Exact validated password for the blocking hash operation.
    pub password: String,
}

/// Validated login data sharing the same name and password rules as registration.
pub struct Login {
    /// Normalized lowercase login name for lookup.
    pub username: String,
    /// Exact validated password for the blocking verification operation.
    pub password: String,
}

/// Fixed input failures without any submitted field contents.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InputError {
    /// The login name violates the shared ASCII name contract.
    #[error("Username must contain 3-32 ASCII letters, digits or underscores")]
    Username,
    /// The display name is empty, too long or contains control characters.
    #[error("Display name must contain 1-64 Unicode scalar values without controls")]
    DisplayName,
    /// The password violates the scalar, byte or whitespace limits.
    #[error(
        "Password must contain 8-128 Unicode scalar values, at most 512 bytes, without whitespace"
    )]
    Password,
}

/// Password execution failures, distinct from an ordinary password mismatch.
#[derive(Debug, thiserror::Error)]
pub enum PasswordError {
    /// The operating system could not supply a fresh salt.
    #[error("Password salt generation failed: {0}")]
    Random(#[from] getrandom::Error),
    /// Argon2 parameters could not be constructed.
    #[error("Password parameters are invalid: {0}")]
    Parameters(#[from] argon2::Error),
    /// A stored PHC string or a password operation failed.
    #[error("Password hash operation failed: {0}")]
    Hash(#[from] password_hash::Error),
    /// The independently executed blocking operation did not complete normally.
    #[error("Password task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

/// Normalize registration and login names using only surrounding ASCII whitespace.
pub fn normalize_username(username: &str) -> Result<String, InputError> {
    let username = username.trim_matches(|character: char| character.is_ascii_whitespace());
    if !(3..=32).contains(&username.len())
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(InputError::Username);
    }
    Ok(username.to_ascii_lowercase())
}

/// Validate an explicit trimmed display name or use the already normalized username.
pub fn normalize_display_name(
    display_name: Option<&str>,
    username: &str,
) -> Result<String, InputError> {
    let name = display_name.unwrap_or(username).trim();
    if !(1..=64).contains(&name.chars().count()) || name.chars().any(char::is_control) {
        return Err(InputError::DisplayName);
    }
    Ok(name.to_owned())
}

/// Validate the exact password without trimming, normalizing or replacing any bytes.
pub fn validate_password(password: &str) -> Result<(), InputError> {
    if password.len() > 512
        || !(8..=128).contains(&password.chars().count())
        || password.chars().any(char::is_whitespace)
    {
        return Err(InputError::Password);
    }
    Ok(())
}

/// Hash a previously validated password off the async executor with a fresh 16-byte salt.
pub async fn hash_password(password: String) -> Result<String, PasswordError> {
    tokio::task::spawn_blocking(move || {
        let mut salt = [0_u8; 16];
        getrandom::fill(&mut salt)?;
        let salt = SaltString::encode_b64(&salt)?;
        let params = Params::new(19_456, 2, 1, Some(32))?;
        Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password(password.as_bytes(), &salt)?
            .to_string())
    })
    .await?
}

/// Verify using stored PHC parameters off the async executor; only a mismatch is false.
pub async fn verify_password(password: String, encoded: String) -> Result<bool, PasswordError> {
    tokio::task::spawn_blocking(move || {
        let hash = PasswordHash::new(&encoded)?;
        match Argon2::default().verify_password(password.as_bytes(), &hash) {
            Ok(()) => Ok(true),
            Err(password_hash::Error::Password) => Ok(false),
            Err(error) => Err(error.into()),
        }
    })
    .await?
}

#[cfg(test)]
mod tests {
    //! Contract boundaries for shared account validation and password calculations.

    use super::*;

    /// Login names trim ASCII whitespace only and preserve the agreed character limits.
    #[test]
    fn username_normalization_and_boundaries() {
        for (input, normalized) in [(" \tTeST_1\r\n", "test_1"), ("Ab3", "ab3")] {
            assert_eq!(normalize_username(input).unwrap(), normalized);
        }
        assert!(normalize_username(&"A".repeat(32)).is_ok());
        for input in [
            "",
            "ab",
            "a-b",
            "中文名",
            "a b",
            "\u{a0}test\u{a0}",
            &"a".repeat(33),
        ] {
            assert_eq!(normalize_username(input), Err(InputError::Username));
        }
    }

    /// Display names count scalars while passwords retain exact Unicode and reject whitespace.
    #[test]
    fn display_name_and_password_boundaries() {
        assert_eq!(normalize_display_name(None, "test").unwrap(), "test");
        assert_eq!(
            normalize_display_name(Some(" \u{3000}张三 \n"), "test").unwrap(),
            "张三"
        );
        assert!(normalize_display_name(Some(&"😀".repeat(64)), "test").is_ok());
        for name in ["", " \t", "a\nb", "a\u{7f}b", &"😀".repeat(65)] {
            assert_eq!(
                normalize_display_name(Some(name), "test"),
                Err(InputError::DisplayName)
            );
        }
        for password in [
            "12345678",
            "密码测试密码测试",
            &"😀".repeat(128),
            "a\u{301}a\u{301}a\u{301}a\u{301}",
        ] {
            assert_eq!(validate_password(password), Ok(()));
        }
        for password in [
            "1234567",
            &"😀".repeat(129),
            "1234 678",
            "1234\t678",
            "1234\n678",
            "1234\u{a0}678",
            "1234\u{3000}678",
        ] {
            assert_eq!(validate_password(password), Err(InputError::Password));
        }
    }

    /// Hashes contain the fixed baseline and fresh salts; malformed hashes remain errors.
    #[tokio::test]
    async fn password_hash_roundtrip_and_failures() {
        let password = "Acorn-密码-😀";
        let first = hash_password(password.into()).await.unwrap();
        let second = hash_password(password.into()).await.unwrap();
        assert_ne!(first, second);
        let parsed = PasswordHash::new(&first).unwrap();
        assert_eq!(parsed.algorithm.as_str(), "argon2id");
        assert_eq!(parsed.version, Some(19));
        assert_eq!(parsed.params.get_decimal("m"), Some(19_456));
        assert_eq!(parsed.params.get_decimal("t"), Some(2));
        assert_eq!(parsed.params.get_decimal("p"), Some(1));
        assert_eq!(parsed.hash.unwrap().len(), 32);
        assert_ne!(parsed.salt, PasswordHash::new(&second).unwrap().salt);
        assert!(
            verify_password(password.into(), first.clone())
                .await
                .unwrap()
        );
        assert!(
            !verify_password("wrong-password".into(), first)
                .await
                .unwrap()
        );
        assert!(matches!(
            verify_password(password.into(), "invalid-phc".into()).await,
            Err(PasswordError::Hash(_))
        ));
    }
}
