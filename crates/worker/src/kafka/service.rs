//! Independent task polling, single-slot execution and bounded FIFO result handoff.

use super::{
    handoff::{self, Entry, Origin},
    ownership::{Context as ConsumerContext, Ownership},
};
use crate::{config::Config, rpc::WorkerService};
use acorn_protocol::{
    records,
    worker::v1::{GetJobReply, JobFailure, JobPhase, JobState, get_job_reply::Terminal},
};
use anyhow::{Context, Result, anyhow};
use rdkafka::{
    Message, Offset,
    consumer::{Consumer, StreamConsumer},
    error::KafkaError,
    message::OwnedMessage,
    producer::FutureProducer,
};
use std::{
    collections::{HashMap, VecDeque},
    future::{Future, pending},
    pin::Pin,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
use tokio::{sync::watch, task::JoinHandle, time::Instant};

/// One executing task; its result position is reserved before execution begins.
struct Active {
    /// Original task position and assignment.
    origin: Origin,
    /// Latest bounded snapshot from the common execution runtime.
    updates: watch::Receiver<GetJobReply>,
}

/// A retained runtime result may only deduplicate records under its original assignment.
struct Retained {
    /// Assignment under which this execution was admitted.
    lease: Arc<AtomicBool>,
    /// Channel identity prevents an older handoff from removing a newer execution.
    updates: watch::Receiver<GetJobReply>,
}

/// Completion of the single in-flight event send or terminal handoff.
enum Sent {
    /// Terminal handoff completed and its runtime retention can be released.
    Terminal(Box<Entry>),
    /// A finite progress send reports to the driver that owns its failure category.
    Running {
        /// Execution whose snapshots share one failure observation.
        job_id: String,
        /// Publication outcome; only temporary transport failures may be skipped.
        result: Result<()>,
    },
}

/// At most one finite event send or terminal handoff is in flight.
type Sending = Pin<Box<dyn Future<Output = Result<Sent>> + Send>>;
/// The existing sandbox shutdown future, polled alongside the consumer.
type Stopping = Pin<Box<dyn Future<Output = Result<()>> + Send>>;

/// Callable Worker Kafka service; the binary entry point is switched in the lifecycle task.
pub struct KafkaWorker {
    /// Common runtime, also exposed through the retained status RPC.
    worker: WorkerService,
    /// Validated deployment configuration.
    config: Config,
    /// Consumer with callbacks that invalidate original partition leases.
    consumer: Arc<StreamConsumer<ConsumerContext>>,
    /// Producer remains available when a blocked executor leaves the task group.
    producer: FutureProducer,
    /// Original assignment identities and driver notifications.
    ownership: Ownership,
}

impl KafkaWorker {
    /// Subscribe to tasks and own the caller's event producer built from the same configuration.
    pub fn new(worker: WorkerService, producer: FutureProducer) -> Result<Self> {
        let config = worker.shared.config.clone();
        let ownership = Ownership::new();
        let consumer: StreamConsumer<ConsumerContext> = super::consumer_config(&config.kafka)
            .create_with_context(ConsumerContext {
                ownership: ownership.clone(),
            })?;
        let consumer = Arc::new(consumer);
        consumer.subscribe(&[&config.kafka.tasks_topic])?;
        Ok(Self {
            worker,
            config,
            consumer,
            producer,
            ownership,
        })
    }

    /// Poll the group throughout execution, publishing and shutdown without buffering tasks.
    pub async fn run(self, mut stop: watch::Receiver<bool>) -> Result<()> {
        let Self {
            worker,
            config,
            consumer,
            producer,
            ownership,
        } = self;
        let mut consumer = Some(consumer);
        let mut assignment = ownership.changed.subscribe();
        let mut active: Option<Active> = None;
        let mut results = VecDeque::new();
        let mut retained: HashMap<String, Retained> = HashMap::new();
        let mut latest: Option<GetJobReply> = None;
        let mut sending: Option<Sending> = None;
        let mut sending_terminal = false;
        let mut stopping: Option<Stopping> = None;
        let mut stopped = false;
        let mut shutdown_started = false;
        let mut deadline = None;
        let (message_deadline, _) = watch::channel(None);
        let mut closing: Option<JoinHandle<Result<()>>> = None;
        let mut paused = None;
        let mut failure = None;
        let mut last_task_error = None;
        let mut last_running_error = None;
        let result: Result<()> = async {
            loop {
                worker.shared.jobs.lock().expect("Job state lock poisoned").mq_results =
                    results.len() + usize::from(sending_terminal);
                if *stop.borrow() && !shutdown_started {
                    shutdown_started = true;
                    latest = None;
                    let worker = worker.clone();
                    stopping = Some(Box::pin(async move { worker.shutdown().await }));
                }
                if worker.blocked() && consumer.is_some() {
                    begin_close(&mut consumer, &ownership, &mut closing);
                    paused = None;
                }
                let occupied = results.len() + usize::from(active.is_some()) + usize::from(sending_terminal);
                let accept = !shutdown_started && !worker.blocked() && active.is_none() && occupied < config.results.max_unacknowledged_results;
                if let Some(consumer) = &consumer
                    && (paused != Some(!accept) || assignment.has_changed()?) {
                        assignment.borrow_and_update();
                        let partitions = consumer.assignment()?;
                        if accept { consumer.resume(&partitions)?; } else { consumer.pause(&partitions)?; }
                        paused = Some(!accept);
                }
                if sending.is_none() {
                    if let Some(entry) = results.pop_front() {
                        let producer = producer.clone();
                        let kafka = config.kafka.clone();
                        let weak = consumer.as_ref().map(Arc::downgrade).unwrap_or_default();
                        let ownership = ownership.clone();
                        let changes = ownership.changed.subscribe();
                        let shutdown = message_deadline.subscribe();
                        sending_terminal = true;
                        let publish_config = kafka;
                        sending = Some(Box::pin(async move {
                            handoff::terminal(entry,
                                move |reply| {
                                    let producer = producer.clone();
                                    let kafka = publish_config.clone();
                                    async move { handoff::publish(&producer, &kafka.events_topic, &reply, kafka.max_event_record_bytes, Duration::from_millis(kafka.queue_timeout_ms)).await }
                                },
                                move |origin| {
                                    handoff::commit(&weak, &ownership, &origin, *shutdown.borrow())
                                },
                                changes,
                            ).await.map(|entry| Sent::Terminal(Box::new(entry)))
                        }));
                    } else if let Some(reply) = latest.take() {
                        let producer = producer.clone();
                        let kafka = config.kafka.clone();
                        sending = Some(Box::pin(async move {
                            let result = handoff::publish(&producer, &kafka.events_topic, &reply, kafka.max_event_record_bytes, Duration::from_millis(kafka.queue_timeout_ms)).await;
                            Ok(Sent::Running { job_id: reply.job_id, result })
                        }));
                    }
                }
                if stopped && active.is_none() && results.is_empty() && sending.is_none() {
                    if consumer.is_some() { begin_close(&mut consumer, &ownership, &mut closing); }
                    if closing.is_none() { break; }
                }
                tokio::select! {
                    changed = stop.changed(), if !shutdown_started => {
                        if changed.is_err() { return Err(anyhow!("KafkaWorkerStopChannelClosed")); }
                    }
                    _ = assignment.changed() => { paused = None; }
                    received = receive(consumer.as_deref()) => {
                        let message = match received {
                            Ok(message) => {
                                if last_task_error.take().is_some() {
                                    tracing::info!("Kafka Worker task transport recovered");
                                }
                                message
                            }
                            Err(error) if acorn_kafka::temporary(&error) => {
                                let code = error.rdkafka_error_code();
                                if last_task_error != code {
                                    tracing::warn!(?code, %error, "Kafka Worker task transport unavailable");
                                }
                                last_task_error = code;
                                continue;
                            }
                            Err(error) => return Err(error).context("KafkaWorkerTaskReceiveFailed"),
                        };
                        let topic = message.topic().to_owned();
                        let partition = message.partition();
                        let offset = message.offset();
                        let lease = ownership.current.lock().expect("Kafka assignment lock poisoned").get(&(topic.clone(), partition)).cloned();
                        let Some(lease) = lease else { continue; };
                        if !accept {
                            // A newly assigned partition may deliver before the driver pauses it.
                            // Rewind this unadmitted record instead of adding a pending-task queue.
                            consumer.as_ref().expect("receiving consumer").seek(&topic, partition, Offset::Offset(offset), Duration::ZERO)?;
                            paused = None;
                            continue;
                        }
                        let admitted: Result<()> = (|| {
                            let payload = message.payload().context("KafkaRecordDecodeFailed: missing task payload")?;
                            let request = records::decode_task(payload, config.kafka.max_task_record_bytes)?;
                            worker.validate_id(&request.job_id).map_err(|_| anyhow!("InvalidJobId"))?;
                            let origin = Origin { topic: topic.clone(), partition, offset, lease };
                            let id = request.job_id.clone();
                            let total_cases = request.cases.len() as u32;
                            if let Some(previous) = retained.get(&id)
                                && !Arc::ptr_eq(&previous.lease, &origin.lease) {
                                // A new assignment must execute its own record, even when an old
                                // execution's terminal is still waiting for publication.
                                worker.forget(&previous.updates);
                                retained.remove(&id);
                            }
                            match worker.admit(request) {
                                Ok(admitted) => {
                                    let initial = admitted.borrow().clone();
                                    if initial.state == JobState::Running as i32 { latest = Some(initial); }
                                    retained.insert(id, Retained { lease: origin.lease.clone(), updates: admitted.clone() });
                                    active = Some(Active { origin, updates: admitted });
                                }
                                Err(error) if error.code() == tonic::Code::InvalidArgument => {
                                    results.push_back(Entry { reply: GetJobReply {
                                        job_id: id, state: JobState::Failed as i32, phase: JobPhase::Done as i32,
                                        completed_cases: 0, total_cases,
                                        terminal: Some(Terminal::Failure(JobFailure { code: error.message().into(), diagnostic: "Task content violates Worker admission limits".into() })),
                                    }, origin, updates: None });
                                }
                                Err(error) => return Err(anyhow!("KafkaWorkerAdmissionFailed: {}", error.message())),
                            }
                            Ok(())
                        })();
                        if let Err(error) = admitted {
                            tracing::error!(%topic, partition, offset, %error, "Kafka Worker task rejected");
                            return Err(error);
                        }
                    }
                    update = progress(&mut active) => {
                        update?;
                        let current = active.as_mut().expect("active progress source");
                        let reply = current.updates.borrow_and_update().clone();
                        if reply.state == JobState::Running as i32 {
                            if !shutdown_started { latest = Some(reply); }
                        } else {
                            latest = None;
                            let current = active.take().expect("completed active task");
                            results.push_back(Entry { reply, origin: current.origin, updates: Some(current.updates) });
                        }
                    }
                    result = send(&mut sending) => {
                        sending = None;
                        sending_terminal = false;
                        match result? {
                            Sent::Terminal(entry) => {
                                if last_running_error.as_ref().is_some_and(|(job, _)| job == &entry.reply.job_id) {
                                    last_running_error = None;
                                }
                                if let Some(updates) = &entry.updates {
                                    worker.forget(updates);
                                    if retained.get(&entry.reply.job_id).is_some_and(|previous| previous.updates.same_channel(updates)) {
                                        retained.remove(&entry.reply.job_id);
                                    }
                                }
                            }
                            Sent::Running { job_id, result } => match result {
                                Ok(()) => {
                                    if last_running_error.take().is_some_and(|(job, _)| job == job_id) {
                                        tracing::info!(job = %job_id, "Kafka Worker running snapshot transport recovered");
                                    }
                                }
                                Err(error) if error.downcast_ref::<KafkaError>().is_some_and(acorn_kafka::temporary) => {
                                    let code = error.downcast_ref::<KafkaError>().and_then(KafkaError::rdkafka_error_code);
                                    if last_running_error.as_ref().is_none_or(|(job, previous)| job != &job_id || *previous != code) {
                                        tracing::warn!(job = %job_id, ?code, %error, "Kafka Worker running snapshot unavailable");
                                    }
                                    last_running_error = Some((job_id, code));
                                }
                                Err(error) => return Err(error),
                            }
                        }
                    }
                    result = finish_execution(&mut stopping) => {
                        stopping = None;
                        stopped = true;
                        if let Err(error) = result { failure = Some(error); }
                        deadline = Some(Instant::now() + Duration::from_millis(config.kafka.shutdown_timeout_ms));
                        message_deadline.send_replace(deadline);
                    }
                    result = finish_close(&mut closing) => {
                        closing = None;
                        result?;
                    }
                    _ = expire(deadline) => return Err(anyhow!("KafkaWorkerShutdownTimedOut: message handoff or consumer close incomplete")),
                }
            }
            Ok(())
        }.await;
        if result.is_err() {
            ownership.revoke_all();
        }
        // Faults use the same execution stop path. The execution future is never aborted.
        let execution = if let Some(stopping) = stopping {
            stopping.await
        } else if !shutdown_started {
            worker.shutdown().await
        } else {
            Ok(())
        };
        drop(sending);
        drop(results);
        worker
            .shared
            .jobs
            .lock()
            .expect("Job state lock poisoned")
            .mq_results = 0;
        begin_close(&mut consumer, &ownership, &mut closing);
        let close = if let Some(task) = closing {
            let close_deadline = deadline.unwrap_or_else(|| {
                Instant::now() + Duration::from_millis(config.kafka.shutdown_timeout_ms)
            });
            match tokio::time::timeout_at(close_deadline, task).await {
                Ok(result) => result
                    .context("KafkaWorkerConsumerCloseFailed")
                    .and_then(|result| result),
                Err(error) => Err(error).context("KafkaWorkerConsumerCloseTimedOut"),
            }
        } else {
            Ok(())
        };
        result
            .and(execution)
            .and(close)
            .and(failure.map_or(Ok(()), Err))
    }
}

/// Keep polling even when all assigned partitions are paused; no task queue is introduced.
async fn receive(
    consumer: Option<&StreamConsumer<ConsumerContext>>,
) -> std::result::Result<OwnedMessage, rdkafka::error::KafkaError> {
    match consumer {
        Some(consumer) => consumer.recv().await.map(|message| message.detach()),
        None => pending().await,
    }
}

/// Observe the next runtime update, including an already completed duplicate record.
async fn progress(active: &mut Option<Active>) -> Result<()> {
    match active {
        Some(active) if active.updates.borrow().state != JobState::Running as i32 => Ok(()),
        Some(active) => active
            .updates
            .changed()
            .await
            .context("KafkaWorkerExecutionUpdatesClosed"),
        None => pending().await,
    }
}

/// Poll the independent bounded event publication and terminal handoff.
async fn send(sending: &mut Option<Sending>) -> Result<Sent> {
    match sending {
        Some(sending) => sending.await,
        None => pending().await,
    }
}

/// Continue consumer polling while the existing sandbox shutdown path runs.
async fn finish_execution(stopping: &mut Option<Stopping>) -> Result<()> {
    match stopping {
        Some(stopping) => stopping.await,
        None => pending().await,
    }
}

/// Close off the runtime so the SDK's synchronous drop never stalls execution or publication.
fn begin_close(
    consumer: &mut Option<Arc<StreamConsumer<ConsumerContext>>>,
    ownership: &Ownership,
    closing: &mut Option<JoinHandle<Result<()>>>,
) {
    if let Some(consumer) = consumer.take() {
        ownership.revoke_all();
        *closing = Some(tokio::spawn(super::close::close(consumer)));
    }
}

/// Observe release of the final consumer handle through the SDK destructor.
async fn finish_close(closing: &mut Option<JoinHandle<Result<()>>>) -> Result<()> {
    match closing {
        Some(closing) => closing.await.context("KafkaWorkerConsumerCloseFailed")?,
        None => pending().await,
    }
}

/// Apply the one total message deadline after sandbox stop has completed.
async fn expire(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => pending().await,
    }
}
