use std::io::SeekFrom;

use chrono::{DateTime, TimeDelta, Utc};
use dirs::data_dir;
use smol::{
    fs::{File, OpenOptions},
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
};

use crate::tags::Tags;

pub struct Log {
    file: File,
    languages: Tags,
    projects: Tags,
    current_open: u128,
    last_idxs: Option<(usize, usize)>,
}

impl Log {
    pub async fn new(
        heartbeat_timestamp: Option<DateTime<Utc>>,
        languages: Tags,
        projects: Tags,
    ) -> Self {
        let mut file_path = data_dir().expect("Failed to find data directory");
        file_path.push("code-statistics");
        file_path.push("log");

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .append(true)
            .open(file_path)
            .await
            .expect("Failed to open log file");
        if file.seek(SeekFrom::End(-9)).await.is_ok() {
            let mut buf = [0; 9];
            file.read_exact(&mut buf)
                .await
                .expect("Failed to read old entry from log file");

            if buf[0] != 0 {
                let last_timestamp = DateTime::from_timestamp(
                    // This should always be the correct length so the cast will not fail
                    i64::from_ne_bytes((&buf[1..]).try_into().unwrap()),
                    0,
                )
                .expect("Timestamp from file is invalid")
                    + TimeDelta::seconds(1);

                let last_timestamp = if let Some(heartbeat_timestamp) = heartbeat_timestamp {
                    last_timestamp.max(heartbeat_timestamp)
                } else {
                    last_timestamp
                };

                file.write_all(&[0])
                    .await
                    .expect("Failed to write to log file");
                file.write_all(&last_timestamp.timestamp().to_ne_bytes())
                    .await
                    .expect("Failed to write to log file");
            }
        }

        Self {
            file,
            languages,
            projects,
            current_open: 0,
            last_idxs: None,
        }
    }

    pub async fn start_event(&mut self, language: &str, project: &str) -> u128 {
        let language_idx = self.languages.get(language).await + 1;
        let project_idx = self.projects.get(project).await;

        if !self
            .last_idxs
            .is_some_and(|(last_lang_idx, last_proj_idx)| {
                last_lang_idx == language_idx && last_proj_idx == project_idx
            })
        {
            self.file
                .write_all(&[language_idx
                    .try_into()
                    .expect("Cannot support more that 255 languages")])
                .await
                .expect("Failed to write to log file");
            self.file
                .write_all(
                    &TryInto::<u16>::try_into(project_idx)
                        .expect("Cannot support more than 65536 projects")
                        .to_ne_bytes(),
                )
                .await
                .expect("Failed to write to log file");
            self.file
                .write_all(&Utc::now().timestamp().to_ne_bytes())
                .await
                .expect("Failed to write to log file");
        }

        self.last_idxs = Some((language_idx, project_idx));

        self.current_open += 1;
        self.current_open
    }

    pub async fn stop_event(&mut self, id: u128) {
        if id == self.current_open {
            self.file
                .write_all(&[0])
                .await
                .expect("Failed to write to log file");
            self.file
                .write_all(&Utc::now().timestamp().to_ne_bytes())
                .await
                .expect("Failed to write to log file");

            self.last_idxs = None;
        }
    }
}
