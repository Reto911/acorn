//! Kafka payload boundaries and exact preservation of existing Protobuf messages.

use acorn_protocol::{
    records::{self, MAX_EVENT_RECORD_BYTES, MAX_TASK_RECORD_BYTES, RecordError},
    worker::v1::{
        GetJobReply, JobFailure, JobPhase, JobState, RunLimits, SubmitJobRequest, TestCase,
        get_job_reply::Terminal,
    },
};
use prost::Message;

/// Source and ordered case bytes survive encoding without text normalization or envelopes.
#[test]
fn task_bytes_and_derived_key_are_preserved() {
    let task = SubmitJobRequest {
        job_id: "任务-1".into(),
        language_id: "python".into(),
        source: b"print('hello')\r\n".to_vec(),
        cases: vec![
            TestCase {
                input: vec![0, 255, 13, 10],
                expected_output: vec![],
            },
            TestCase {
                input: vec![],
                expected_output: vec![255, 0, 10],
            },
        ],
        run_limits: Some(RunLimits {
            memory_kib: 1,
            ..Default::default()
        }),
    };
    let encoded = records::encode_task(&task, MAX_TASK_RECORD_BYTES).unwrap();
    assert_eq!(encoded.key().as_bytes(), task.job_id.as_bytes());
    assert_eq!(encoded.payload(), task.encode_to_vec());
    assert_eq!(
        records::decode_task(encoded.payload(), encoded.payload().len()).unwrap(),
        task
    );
    assert!(matches!(
        records::encode_task(&task, encoded.payload().len() - 1),
        Err(RecordError::TooLarge { .. })
    ));
    assert!(matches!(
        records::decode_task(encoded.payload(), encoded.payload().len() - 1),
        Err(RecordError::TooLarge { .. })
    ));
}

/// Running phases and failure terminals retain their existing message shape.
#[test]
fn event_shapes_and_encoded_size_boundaries() {
    for phase in [
        JobPhase::Preparing,
        JobPhase::Compiling,
        JobPhase::RunningCase,
        JobPhase::Checking,
        JobPhase::Done,
    ] {
        let event = GetJobReply {
            job_id: "submission-1".into(),
            state: if phase == JobPhase::Done {
                JobState::Failed
            } else {
                JobState::Running
            }
            .into(),
            phase: phase.into(),
            completed_cases: 1,
            total_cases: 2,
            terminal: (phase == JobPhase::Done).then(|| {
                Terminal::Failure(JobFailure {
                    code: "InvalidJob".into(),
                    diagnostic: "Invalid resource limit".into(),
                })
            }),
        };
        let size = event.encoded_len();
        let encoded = records::encode_event(&event, size).unwrap();
        assert_eq!(encoded.key(), event.job_id);
        assert_eq!(
            records::decode_event(encoded.payload(), size).unwrap(),
            event
        );
        assert!(matches!(
            records::encode_event(&event, size - 1),
            Err(RecordError::TooLarge { .. })
        ));
        assert!(matches!(
            records::decode_event(encoded.payload(), size - 1),
            Err(RecordError::TooLarge { .. })
        ));
    }
}

/// Protocol maxima remain effective even when callers accidentally supply larger limits.
#[test]
fn protocol_ceiling_and_malformed_payloads_are_rejected() {
    let oversized = SubmitJobRequest {
        source: vec![0; MAX_TASK_RECORD_BYTES],
        ..Default::default()
    };
    assert!(matches!(
        records::encode_task(&oversized, usize::MAX),
        Err(RecordError::TooLarge { .. })
    ));
    assert!(matches!(
        records::decode_task(&vec![0; MAX_TASK_RECORD_BYTES + 1], usize::MAX),
        Err(RecordError::TooLarge { .. })
    ));
    let oversized = GetJobReply {
        job_id: "x".repeat(MAX_EVENT_RECORD_BYTES),
        ..Default::default()
    };
    assert!(matches!(
        records::encode_event(&oversized, usize::MAX),
        Err(RecordError::TooLarge { .. })
    ));
    assert!(matches!(
        records::decode_event(&vec![0; MAX_EVENT_RECORD_BYTES + 1], usize::MAX),
        Err(RecordError::TooLarge { .. })
    ));
    for error in [
        records::decode_task(&[0xff], MAX_TASK_RECORD_BYTES).unwrap_err(),
        records::decode_event(&[0xff], MAX_EVENT_RECORD_BYTES).unwrap_err(),
    ] {
        assert!(matches!(error, RecordError::Decode(_)));
        assert!(error.to_string().starts_with("KafkaRecordDecodeFailed"));
    }
}
