//! Account persistence using the existing users table and shared operation boundaries.

use sqlx::Row;

use crate::auth::User;

use super::{Database, Error, Result, mysql_error_number, now_ms};

/// Owned login data; never serialized or debug-formatted with the stored password hash.
pub struct UserCredentials {
    /// Public account identity for successful authentication.
    pub user: User,
    /// Complete PHC string used after the database connection has been returned.
    pub password_hash: String,
}

impl Database {
    /// Read a previously normalized login name, returning owned data and releasing the connection.
    pub async fn user_by_username(&self, username: &str) -> Result<Option<UserCredentials>> {
        self.observe("Read user credentials", false, async {
            let row = sqlx::query("SELECT id, username_normalized, display_name, password_hash FROM users WHERE username_normalized = ?")
                .bind(username).fetch_optional(&self.pool).await?;
            row.map(|row| {
                Ok(UserCredentials {
                    user: User {
                        id: row.try_get("id")?,
                        username: row.try_get("username_normalized")?,
                        display_name: row.try_get("display_name")?,
                    },
                    password_hash: row.try_get("password_hash")?,
                })
            }).transpose()
        }).await
    }

    /// Insert validated account fields and a precomputed PHC hash without accepting an identity or role.
    pub async fn create_user(
        &self,
        username: &str,
        display_name: &str,
        password_hash: &str,
    ) -> Result<User> {
        self.observe("Create user", true, async {
            let inserted = sqlx::query("INSERT INTO users (username_normalized, display_name, password_hash, role, status, created_at_ms) VALUES (?, ?, ?, 'student', 'active', ?)")
                .bind(username).bind(display_name).bind(password_hash).bind(now_ms()?)
                .execute(&self.pool).await;
            let result = match inserted {
                Ok(result) => result,
                // With the ID auto-assigned, username is this insert's only unique input.
                Err(error) if mysql_error_number(&error) == Some(1062) => return Err(Error::UsernameTaken),
                Err(error) => return Err(error.into()),
            };
            let id = i64::try_from(result.last_insert_id()).map_err(|error| Error::Data(error.into()))?;
            Ok(User { id, username: username.into(), display_name: display_name.into() })
        }).await
    }
}
