use std::{io::SeekFrom, time::Duration};

use chrono::{DateTime, Utc};
use dirs::data_dir;
use smol::{
    fs::OpenOptions,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    LocalExecutor, Timer,
};

pub async fn start_heartbeat(executor: &LocalExecutor<'_>) -> Option<DateTime<Utc>> {
    let mut file_path = data_dir().expect("Failed to get data directory");
    file_path.push("code-statistics");
    file_path.push("heartbeat");

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .read(true)
        .open(file_path)
        .await
        .expect("Failed to open heartbeat directory");

    let mut buf = [0; 8];
    let last_time = if file.read_exact(&mut buf).await.is_ok() {
        Some(DateTime::from_timestamp(i64::from_ne_bytes(buf), 0).expect("Invalid last heartbeat"))
    } else {
        None
    };

    executor
        .spawn(async move {
            loop {
                file.seek(SeekFrom::Start(0))
                    .await
                    .expect("Failed to change position in heartbeat file");
                file.write_all(&Utc::now().timestamp().to_ne_bytes())
                    .await
                    .expect("Failed to write heartbeat");
                Timer::after(Duration::from_secs(10)).await;
            }
        })
        .detach();

    last_time
}
