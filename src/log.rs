use std::io::SeekFrom;

use chrono::{DateTime, Utc};
use dirs::data_dir;
use futures_concurrency::future::Race;
use smol::{
    fs::{File, OpenOptions},
    io::{AsyncSeekExt, AsyncWriteExt},
    stream::StreamExt,
    LocalExecutor, Timer,
};
use tracing::{debug, span, trace, Instrument, Level};

use crate::{
    channel,
    config::Config,
    debounce::{debounce, LogMessage},
    Sender,
};

#[derive(Debug)]
pub enum Status {
    Active {
        time: DateTime<Utc>,
        language: usize,
        project: usize,
    },
    Dormant {
        time: DateTime<Utc>,
    },
}

#[derive(Debug)]
pub enum SystemMessage {
    Suspend { time: DateTime<Utc> },
    Resume,
}

#[derive(Debug)]
pub enum Message {
    Status(Status),
    System(SystemMessage),
    Heartbeat,
}

async fn send_start_event(
    file: &mut File,
    language_idx: usize,
    project_idx: usize,
    timestamp: DateTime<Utc>,
) {
    trace!("sending start event");
    file.write_all(&[(language_idx + 1)
        .try_into()
        .expect("Cannot support more that 255 languages")])
        .await
        .expect("Failed to write to log file");
    file.write_all(
        &TryInto::<u16>::try_into(project_idx)
            .expect("Cannot support more than 65536 projects")
            .to_ne_bytes(),
    )
    .await
    .expect("Failed to write to log file");
    file.write_all(&timestamp.timestamp().to_ne_bytes())
        .await
        .expect("Failed to write to log file");

    file.write_all(&[0])
        .await
        .expect("Failed to write to log file");
    file.write_all(&timestamp.timestamp().to_ne_bytes())
        .await
        .expect("Failed to write to log file");

    file.seek(SeekFrom::End(-(size_of::<i64>() as i64 + 1)))
        .await
        .expect("Failed to write to log file");
}

async fn send_stop_event(file: &mut File, timestamp: DateTime<Utc>) {
    trace!("sending stop event");
    file.seek(SeekFrom::End(-(size_of::<i64>() as i64)))
        .await
        .expect("Failed to write to log file");
    file.write_all(&timestamp.timestamp().to_ne_bytes())
        .await
        .expect("Failed to write to log file");
}

pub fn log(
    executor: &LocalExecutor<'_>,
    config: &'static Config,
) -> (Sender<LogMessage, 5>, Sender<SystemMessage, 5>) {
    let (system_sender, mut system_receiver) = channel();

    let (sender, mut receiver) = debounce(config.debounce_amount, executor);

    let debounce_input = sender.clone();

    executor
        .spawn(
            async move {
                let mut file_path = data_dir().expect("Failed to find data directory");
                file_path.push("code-statistics");
                file_path.push("log");

                let mut file = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .read(true)
                    .open(file_path)
                    .await
                    .expect("Failed to open log file");

                file.seek(SeekFrom::End(0))
                    .await
                    .expect("Failed to write to log file");

                let mut last_message = None;
                let mut suspended = false;

                trace!("completed set up");

                loop {
                    let message = (
                        async { Some(Message::Status(receiver.next().await?)) },
                        async { Some(Message::System(system_receiver.next().await?)) },
                        async {
                            if last_message.is_none() {
                                Timer::never()
                            } else {
                                Timer::after(config.heartbeat_frequency)
                            }
                            .await;

                            Some(Message::Heartbeat)
                        },
                    )
                        .race()
                        .await;

                    let Some(message) = message else {
                        trace!("stopping because a channel disconnected");
                        break;
                    };

                    debug!(?message, ?last_message, suspended);

                    match message {
                        Message::System(SystemMessage::Suspend { time }) => {
                            send_stop_event(&mut file, time).await;
                            last_message = None;
                            suspended = true;
                        }
                        Message::System(SystemMessage::Resume) => {
                            suspended = false;
                            debounce_input.send(LogMessage::ResetStatus);
                        }
                        _ if suspended => {}

                        Message::Heartbeat => {
                            send_stop_event(&mut file, Utc::now()).await;
                            file.seek(SeekFrom::End(-(size_of::<i64>() as i64 + 1)))
                                .await
                                .expect("Failed to write to log file");
                        }

                        Message::Status(Status::Active {
                            time,
                            language,
                            project,
                        }) => {
                            if last_message
                                .is_none_or(|last_message| last_message != (language, project))
                            {
                                send_start_event(&mut file, language, project, time).await;
                                last_message = Some((language, project));
                            }
                        }
                        Message::Status(Status::Dormant { time }) => {
                            send_stop_event(&mut file, time).await;
                            last_message = None;
                        }
                    }
                }
            }
            .instrument(span!(Level::DEBUG, "log task")),
        )
        .detach();

    (sender, system_sender)
}
