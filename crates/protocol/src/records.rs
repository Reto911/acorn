//! Bounded Kafka payloads using the existing Worker Protobuf messages.

use prost::Message;

use crate::worker::v1::{GetJobReply, SubmitJobRequest};

/// Maximum encoded task payload: sixteen MiB, excluding Kafka framing.
pub const MAX_TASK_RECORD_BYTES: usize = 16_777_216;
/// Maximum encoded event payload: one MiB, excluding Kafka framing.
pub const MAX_EVENT_RECORD_BYTES: usize = 1_048_576;

/// Stable transport errors; semantic identity and result checks belong to consumers.
#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    /// Reject an encoded payload before allocating or decoding it.
    #[error("KafkaRecordTooLarge: {actual} bytes exceeds {limit}")]
    TooLarge {
        /// Actual encoded Protobuf byte count.
        actual: usize,
        /// Configured maximum encoded byte count.
        limit: usize,
    },
    /// The payload cannot be decoded as the topic's Protobuf message.
    #[error("KafkaRecordDecodeFailed: {0}")]
    Decode(#[from] prost::DecodeError),
}

/// An encoded message whose UTF-8 key is derived only from its payload's Job ID.
#[derive(Debug)]
pub struct EncodedRecord {
    /// Job ID copied directly from the message being encoded.
    key: String,
    /// Protobuf binary payload, without an additional envelope.
    payload: Vec<u8>,
}

impl EncodedRecord {
    /// Borrow the Kafka key as the original UTF-8 Job ID.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Borrow the exact Protobuf payload for sending.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

/// Encode a complete task and derive its key from the same message.
pub fn encode_task(task: &SubmitJobRequest, limit: usize) -> Result<EncodedRecord, RecordError> {
    encode(task, &task.job_id, limit.min(MAX_TASK_RECORD_BYTES))
}

/// Encode a running snapshot or terminal event and derive its key from that event.
pub fn encode_event(event: &GetJobReply, limit: usize) -> Result<EncodedRecord, RecordError> {
    encode(event, &event.job_id, limit.min(MAX_EVENT_RECORD_BYTES))
}

/// Decode a task within the configured and protocol byte ceilings.
pub fn decode_task(payload: &[u8], limit: usize) -> Result<SubmitJobRequest, RecordError> {
    decode(payload, limit.min(MAX_TASK_RECORD_BYTES))
}

/// Decode an event; the backend then applies its existing semantic validation.
pub fn decode_event(payload: &[u8], limit: usize) -> Result<GetJobReply, RecordError> {
    decode(payload, limit.min(MAX_EVENT_RECORD_BYTES))
}

/// Check actual encoded bytes before allocating a message's output buffer.
fn encode<M: Message>(
    message: &M,
    job_id: &str,
    limit: usize,
) -> Result<EncodedRecord, RecordError> {
    check_size(message.encoded_len(), limit)?;
    Ok(EncodedRecord {
        key: job_id.to_owned(),
        payload: message.encode_to_vec(),
    })
}

/// Check the received bytes before Protobuf parsing and allocation.
fn decode<M: Message + Default>(payload: &[u8], limit: usize) -> Result<M, RecordError> {
    check_size(payload.len(), limit)?;
    Ok(M::decode(payload)?)
}

/// Report a stable oversized-record error without exposing its contents.
fn check_size(actual: usize, limit: usize) -> Result<(), RecordError> {
    if actual > limit {
        return Err(RecordError::TooLarge { actual, limit });
    }
    Ok(())
}
