use std::time::Duration;

use chrono::{DateTime, Utc};
use futures_concurrency::future::Race;
use smol::{stream::StreamExt, LocalExecutor, Timer};
use tracing::{debug, span, trace, Instrument, Level};

use crate::log::Status;

#[derive(Debug)]
pub enum LogMessage {
    Start {
        id: u128,
        time: DateTime<Utc>,
        language: usize,
        project: usize,
    },
    End {
        id: u128,
        time: DateTime<Utc>,
    },
    ResetStatus,
}

pub fn debounce<const S: usize>(
    time: Duration,
    executor: &LocalExecutor<'_>,
) -> (crate::Sender<LogMessage, S>, crate::Receiver<Status, S>) {
    let (input_sender, mut input_receiver) = crate::channel::<_, S>();
    let (output_sender, output_receiver) = crate::channel::<_, S>();
    let (inter_sender, mut inter_receiver) = crate::channel::<_, S>();

    executor
        .spawn(
            async move {
                loop {
                    let Some(mut value) = inter_receiver.next().await else {
                        return;
                    };

                    trace!(event = "new status", ?value);

                    loop {
                        let new_value = (
                            async {
                                Timer::after(time).await;
                                trace!(event = "no activity, sending status", ?value);
                                None
                            },
                            async { Some(inter_receiver.next().await) },
                        )
                            .race()
                            .await;

                        match new_value {
                            Some(Some(new_value)) => {
                                value = new_value;
                                debug!(event = "received new status, restarting timer", new_value = ?value);
                            }
                            Some(None) => return,
                            None => break,
                        }
                    }

                    output_sender.send(value)
                }
            }
            .instrument(span!(Level::DEBUG, "debounce task")),
        )
        .detach();

    executor
        .spawn(async move {
            let mut last_message = None;

            loop {
                let Some(value) = input_receiver.next().await else {
                    return;
                };

                debug!(event = "new log message", ?value);

                match value {
                    LogMessage::Start {
                        id: _,
                        time,
                        language,
                        project,
                    } => {
                        if !matches!(last_message, Some(LogMessage::Start { id: _, time: _, language: old_language, project: old_project }) if old_language == language && old_project == project) {
                            inter_sender.send(Status::Active { time, language, project });
                            debug!(event = "new status", status = ?Status::Active { time, language, project });
                        } else {
                            debug!("ignoring same status");
                        }

                        last_message = Some(value);

                    },
                    LogMessage::End { id, time } => if matches!(last_message, Some(LogMessage::Start { id: old_id, time: _, language: _, project: _ }) if old_id == id) {
                        last_message = Some(value);
                        inter_sender.send(Status::Dormant { time });
                        debug!(event = "new status", status = ?Status::Dormant { time });
                    },
                    LogMessage::ResetStatus => {
                        last_message = None;
                        debug!(event = "resetting last message");
                    },
                }
            }
        }.instrument(span!(Level::DEBUG, "state task")))
        .detach();

    (input_sender, output_receiver)
}
