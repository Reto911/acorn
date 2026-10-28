//! Bounded process-local sessions with synchronized global and per-account LRU indexes.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use lru::LruCache;

use super::User;
use crate::config::AuthConfig;

/// Fixed sliding idle lifetime, also used for the browser cookie.
pub(crate) const IDLE_TIMEOUT: Duration = Duration::from_secs(86_400);

/// Shared session store; its short lock never spans database or password work.
pub(crate) struct Sessions {
    /// Both indexes protected as one invariant.
    indexes: Mutex<Indexes>,
    /// Fixed personal capacity.
    per_user: usize,
    /// Fixed total capacity.
    total: usize,
}

/// One copy of public identity and its last accepted authentication time.
struct Session {
    /// Identity retained by requests already admitted.
    user: User,
    /// Monotonic activity time, updated together with both LRU orders.
    last_activity: Instant,
}

/// Two unbounded LRU containers whose eviction is explicitly coordinated here.
struct Indexes {
    /// Sole owner of session bodies, ordered by last authentication.
    global: LruCache<String, Session>,
    /// Per-account token order without duplicate identity data.
    by_user: HashMap<i64, LruCache<String, ()>>,
}

impl Sessions {
    /// Build empty indexes using already validated startup capacities.
    pub(crate) fn new(config: &AuthConfig) -> Self {
        Self {
            indexes: Mutex::new(Indexes::new()),
            per_user: config.max_sessions_per_user,
            total: config.max_sessions,
        }
    }

    /// Generate outside the lock, then clean, replace, evict and insert atomically.
    pub(crate) fn create(&self, user: User, old: Option<&str>) -> anyhow::Result<String> {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes)?;
        let token = BASE64_URL_SAFE_NO_PAD.encode(bytes);
        let mut indexes = self
            .indexes
            .lock()
            .map_err(|_| anyhow::anyhow!("Session lock is poisoned"))?;
        let now = Instant::now();
        indexes.insert(token.clone(), user, old, now, self.per_user, self.total);
        Ok(token)
    }

    /// Reject expired entries before moving either index or extending the idle lifetime.
    pub(crate) fn authenticate(&self, token: &str) -> anyhow::Result<Option<User>> {
        let mut indexes = self
            .indexes
            .lock()
            .map_err(|_| anyhow::anyhow!("Session lock is poisoned"))?;
        let now = Instant::now();
        Ok(indexes.authenticate(token, now))
    }

    /// Remove the current browser session through the same synchronized deletion path.
    pub(crate) fn remove(&self, token: &str) -> anyhow::Result<()> {
        self.indexes
            .lock()
            .map_err(|_| anyhow::anyhow!("Session lock is poisoned"))?
            .remove(token);
        Ok(())
    }
}

impl Indexes {
    /// Allocate empty containers without automatic independent eviction.
    fn new() -> Self {
        Self {
            global: LruCache::unbounded(),
            by_user: HashMap::new(),
        }
    }

    /// Keep all deletion reasons on one path and discard empty personal indexes.
    fn remove(&mut self, token: &str) {
        if let Some(session) = self.global.pop(token) {
            let personal = self
                .by_user
                .get_mut(&session.user.id)
                .expect("Session has a personal index");
            personal.pop(token);
            if personal.is_empty() {
                self.by_user.remove(&session.user.id);
            }
        }
    }

    /// Authenticate at a lock-owned instant, retaining fixed work for a normal access.
    fn authenticate(&mut self, token: &str, now: Instant) -> Option<User> {
        let session = self.global.peek(token)?;
        if now.duration_since(session.last_activity) >= IDLE_TIMEOUT {
            self.remove(token);
            return None;
        }
        let session = self.global.get_mut(token).expect("Session was just found");
        session.last_activity = now;
        let user = session.user.clone();
        self.by_user
            .get_mut(&user.id)
            .expect("Session has a personal index")
            .get(token);
        Some(user)
    }

    /// Clean the expired prefix, replace this browser and explicitly enforce both capacities.
    fn insert(
        &mut self,
        token: String,
        user: User,
        old: Option<&str>,
        now: Instant,
        per_user: usize,
        total: usize,
    ) {
        while let Some(expired) = self
            .global
            .peek_lru()
            .filter(|(_, session)| now.duration_since(session.last_activity) >= IDLE_TIMEOUT)
            .map(|(token, _)| token.clone())
        {
            self.remove(&expired);
        }
        if let Some(old) = old {
            self.remove(old);
        }
        if let Some(oldest) = self
            .by_user
            .get(&user.id)
            .filter(|personal| personal.len() >= per_user)
            .and_then(|personal| personal.peek_lru())
            .map(|(token, ())| token.clone())
        {
            self.remove(&oldest);
        }
        if self.global.len() >= total {
            let oldest = self
                .global
                .peek_lru()
                .expect("Full cache has an oldest session")
                .0
                .clone();
            self.remove(&oldest);
        }
        self.by_user
            .entry(user.id)
            .or_insert_with(LruCache::unbounded)
            .put(token.clone(), ());
        self.global.put(
            token,
            Session {
                user,
                last_activity: now,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    //! Controlled monotonic instants exercise lifecycle and both index invariants without sleeps.
    use super::*;

    /// Public test identity with no credentials.
    fn user(id: i64) -> User {
        User {
            id,
            username: format!("user{id}"),
            display_name: "User".into(),
        }
    }

    /// Every global entry has exactly one corresponding personal node and no empty index remains.
    fn assert_indexes(indexes: &Indexes) {
        assert_eq!(
            indexes.global.len(),
            indexes.by_user.values().map(LruCache::len).sum::<usize>()
        );
        for (id, personal) in &indexes.by_user {
            assert!(!personal.is_empty());
            for (token, ()) in personal {
                assert_eq!(indexes.global.peek(token).unwrap().user.id, *id);
            }
        }
    }

    /// Sliding renewal accepts the pre-boundary instant and never resurrects an expired token.
    #[test]
    fn expiration_and_sliding_renewal() {
        let mut indexes = Indexes::new();
        let start = Instant::now();
        indexes.insert("one".into(), user(1), None, start, 10, 100);
        let renewed = start + IDLE_TIMEOUT - Duration::from_secs(1);
        assert_eq!(indexes.authenticate("one", renewed), Some(user(1)));
        assert_eq!(indexes.authenticate("one", renewed + IDLE_TIMEOUT), None);
        assert!(indexes.global.is_empty());
        assert!(indexes.by_user.is_empty());
        assert_indexes(&indexes);
    }

    /// Personal eviction follows access order; global eviction removes its personal node too.
    #[test]
    fn personal_then_global_eviction() {
        let mut indexes = Indexes::new();
        let now = Instant::now();
        indexes.insert("a".into(), user(1), None, now, 2, 3);
        indexes.insert("b".into(), user(1), None, now, 2, 3);
        indexes.insert("c".into(), user(2), None, now, 2, 3);
        indexes.authenticate("a", now);
        indexes.insert("d".into(), user(1), None, now, 2, 3);
        assert!(!indexes.global.contains("b"));
        assert!(indexes.global.contains("c"));
        indexes.insert("e".into(), user(3), None, now, 2, 3);
        assert!(!indexes.global.contains("c"));
        assert!(!indexes.by_user.contains_key(&2));
        assert_indexes(&indexes);
    }

    /// Expired prefixes are cleaned before capacity handling, and browser replacement is coordinated.
    #[test]
    fn cleanup_replacement_and_logout() {
        let mut indexes = Indexes::new();
        let start = Instant::now();
        indexes.insert("a".into(), user(1), None, start, 2, 3);
        indexes.insert("b".into(), user(2), None, start, 2, 3);
        indexes.insert(
            "c".into(),
            user(3),
            None,
            start + Duration::from_secs(1),
            2,
            3,
        );
        indexes.insert("d".into(), user(4), Some("c"), start + IDLE_TIMEOUT, 2, 3);
        assert_eq!(indexes.global.len(), 1);
        assert_eq!(indexes.by_user.len(), 1);
        indexes.remove("d");
        indexes.remove("d");
        assert_indexes(&indexes);
        assert!(indexes.global.is_empty());
    }
}
