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
use tracing::debug;

use crate::{channel, config::Config, debounce::debounce, Sender};

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
    Heartbeat,
}

async fn send_start_event(
    file: &mut File,
    language_idx: usize,
    project_idx: usize,
    timestamp: DateTime<Utc>,
) {
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
    file.seek(SeekFrom::End(-(size_of::<i64>() as i64)))
        .await
        .expect("Failed to write to log file");
    file.write_all(&timestamp.timestamp().to_ne_bytes())
        .await
        .expect("Failed to write to log file");
}

pub fn log(executor: &LocalExecutor<'_>, config: &'static Config) -> Sender<LogMessage, 5> {
    let (sender, receiver) = channel();

    let mut receiver = debounce(config.debounce_amount, receiver, executor);

    executor
        .spawn(async move {
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

            let mut current_id = None;
            let mut last_message = None;

            loop {
                let message = if current_id.is_none() {
                    receiver.next().await
                } else {
                    (receiver.next(), async {
                        Timer::after(config.heartbeat_frequency).await;
                        Some(LogMessage::Heartbeat)
                    })
                        .race()
                        .await
                };

                let Some(message) = message else {
                    break;
                };

                debug!(?message);

                match message {
                    LogMessage::Start {
                        id,
                        time,
                        language,
                        project,
                    } => {
                        if last_message
                            .is_none_or(|last_message| last_message != (language, project))
                        {
                            send_start_event(&mut file, language, project, time).await;
                        }
                        current_id = Some(id);
                        last_message = Some((language, project));
                    }
                    LogMessage::End { id, time } => {
                        last_message = None;
                        if current_id.is_some_and(|current_id| current_id == id) {
                            send_stop_event(&mut file, time).await;
                            current_id = None;
                        }
                    }
                    LogMessage::Heartbeat => {
                        if current_id.is_some() {
                            send_stop_event(&mut file, Utc::now()).await;
                            file.seek(SeekFrom::End(-(size_of::<i64>() as i64 + 1)))
                                .await
                                .expect("Failed to write to log file");
                        }
                    }
                }
            }
        })
        .detach();

    sender
}
