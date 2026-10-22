//! Single source of Kafka budget and encoded-record defaults for both TOML loaders.

use acorn_protocol::records::{MAX_EVENT_RECORD_BYTES, MAX_TASK_RECORD_BYTES};

/// Default value for Kafka delivery_timeout_ms.
pub fn delivery_timeout_ms() -> u64 {
    30_000
}

/// Default value for Kafka queue_timeout_ms.
pub fn queue_timeout_ms() -> u64 {
    1_000
}

/// Default value for Kafka shutdown_timeout_ms.
pub fn shutdown_timeout_ms() -> u64 {
    5_000
}

/// Default value for Kafka max_task_record_bytes.
pub fn max_task_record_bytes() -> usize {
    MAX_TASK_RECORD_BYTES
}

/// Default value for Kafka max_event_record_bytes.
pub fn max_event_record_bytes() -> usize {
    MAX_EVENT_RECORD_BYTES
}
