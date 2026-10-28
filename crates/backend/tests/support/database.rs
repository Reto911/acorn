//! Shared independently initialized MySQL fixture for storage and workflow checks.

use acorn_backend::{
    config::{Config, DatabaseConfig},
    database::{Database, initialize_session},
};
use serde::Deserialize;
use sqlx::{AssertSqlSafe, Connection, MySqlConnection, Row};
use uuid::Uuid;

/// Non-secret test connection settings, supplied separately from deployment TOML.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TestSettings {
    /// Development account and pool budgets; name is the owned test database prefix.
    database: DatabaseConfig,
}

/// One independent MySQL database and the application handle that uses it.
pub struct Fixture {
    /// Deployment-aligned settings with this invocation's unique database name.
    pub config: Config,
    /// Actual bounded application connection pool.
    pub database: Database,
}

impl Fixture {
    /// Create and seed one owned database using the exact deployment SQL scripts.
    pub async fn new() -> Self {
        let path = std::env::var("ACORN_TEST_DATABASE_CONFIG")
            .expect("Set ACORN_TEST_DATABASE_CONFIG to a MySQL test TOML file");
        let text = std::fs::read_to_string(path).expect("Read MySQL test TOML");
        let settings: TestSettings = toml::from_str(&text)
            .unwrap_or_else(|error| panic!("Invalid MySQL test TOML at {:?}", error.span()));
        let mut config =
            Config::parse(include_str!("../../../../config/backend.example.toml")).unwrap();
        config.database = settings.database;
        let prefix = &config.database.name;
        assert!(
            !prefix.is_empty()
                && prefix.len() <= 32
                && prefix.bytes().all(|byte| byte.is_ascii_alphanumeric()),
            "Test database.name must be a 1-32 character ASCII alphanumeric prefix"
        );
        config.database.name = format!("{prefix}{}", Uuid::new_v4().simple());
        config.validate().unwrap();
        let mut connection =
            MySqlConnection::connect_with(&config.database.connect_options().unwrap())
                .await
                .unwrap();
        // Only the validated alphanumeric prefix and generated UUID enter the identifier.
        sqlx::query(AssertSqlSafe(format!(
            "CREATE DATABASE `{}` CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin",
            config.database.name
        )))
        .execute(&mut connection)
        .await
        .unwrap();
        sqlx::raw_sql(AssertSqlSafe(format!("USE `{}`", config.database.name)))
            .execute(&mut connection)
            .await
            .unwrap();
        for script in [
            include_str!("../../../../sql/001-initial-schema.sql"),
            include_str!("../../../../examples/seed-user.sql"),
            include_str!("../../../../sql/002-example-problem.sql"),
        ] {
            sqlx::raw_sql(script)
                .execute(&mut connection)
                .await
                .unwrap();
        }
        connection.close().await.unwrap();
        let database = Database::open(&config).await.unwrap();
        Self { config, database }
    }

    /// Open an independent control connection with the application session settings.
    pub async fn connection(&self) -> MySqlConnection {
        let mut connection = MySqlConnection::connect_with(
            &self
                .config
                .database
                .connect_options()
                .unwrap()
                .database(&self.config.database.name),
        )
        .await
        .unwrap();
        initialize_session(&mut connection, self.config.database.lock_wait_timeout_secs)
            .await
            .unwrap();
        connection
    }

    /// Create another account with the shipped development password for ownership checks.
    pub async fn user(&self, username: &str) -> i64 {
        let credentials = self
            .database
            .user_by_username("test")
            .await
            .unwrap()
            .unwrap();
        self.database
            .create_user(username, username, &credentials.password_hash)
            .await
            .unwrap()
            .id
    }

    /// Wait for this fixture's concurrent statements while a control transaction holds a row lock.
    ///
    /// SHOW lists the development account's own sessions without PROCESS privileges.
    /// Inspect only this unique database and keep statement contents out of diagnostics.
    pub async fn wait_for_queries(
        &self,
        control: &mut MySqlConnection,
        prefix: &str,
        count: usize,
    ) {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let rows = sqlx::raw_sql("SHOW FULL PROCESSLIST")
                    .fetch_all(&mut *control)
                    .await
                    .unwrap();
                let matching = rows
                    .iter()
                    .filter(|row| {
                        row.try_get::<Option<String>, _>("db").unwrap().as_deref()
                            == Some(self.config.database.name.as_str())
                            && row
                                .try_get::<Option<String>, _>("Info")
                                .unwrap()
                                .is_some_and(|sql| sql.starts_with(prefix))
                    })
                    .count();
                if matching >= count {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("Concurrent MySQL test statements must reach the controlled row lock");
    }

    /// Close the application pool before deleting only this fixture's owned database.
    pub async fn close(self) {
        self.database.close().await;
        let mut connection =
            MySqlConnection::connect_with(&self.config.database.connect_options().unwrap())
                .await
                .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "DROP DATABASE `{}`",
            self.config.database.name
        )))
        .execute(&mut connection)
        .await
        .unwrap();
        connection.close().await.unwrap();
    }
}
