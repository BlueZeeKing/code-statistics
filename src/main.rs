use core::panic;
use std::{
    os::fd::{FromRawFd, OwnedFd},
    ptr,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use code_statistics::{
    config::read_config, heartbeat::start_heartbeat, log::Log, sd_is_socket_unix, sd_listen_fds,
    tags::Tags, SD_LISTEN_FDS_START,
};
use dirs::data_dir;
use futures_concurrency::future::Race;
use smol::{
    fs::create_dir_all,
    io::{AsyncBufReadExt, BufReader},
    lock::Mutex,
    net::unix::UnixListener,
    stream::StreamExt,
    unblock, Timer,
};

fn main() -> Result<(), ()> {
    let executor = smol::LocalExecutor::new();

    smol::block_on(executor.run(async {
        let data_dir = {
            let mut dir = data_dir().expect("Failed to find data directory");
            dir.push("code-statistics");
            dir
        };
        create_dir_all(&data_dir)
            .await
            .expect("Failed to create data directory");

        let config = Rc::new(read_config().await);
        let timeout = Duration::from_secs_f64(config.timeout);

        let initial_heartbeat = start_heartbeat(
            &executor,
            Duration::from_secs_f64(config.heartbeat_frequency),
        )
        .await;

        let log = Rc::new(Mutex::new(
            Log::new(
                initial_heartbeat,
                Tags::new("languages").await,
                Tags::new("projects").await,
            )
            .await,
        ));

        let fd = unblock(|| unsafe {
            let num_descriptors = sd_listen_fds(1);
            if num_descriptors <= 0 {
                panic!("Failed to get sockets from systemd");
            }
            if sd_is_socket_unix(SD_LISTEN_FDS_START, 1, 1, ptr::null(), 0) != 1 {
                panic!("Wrong kind of socket");
            }
            OwnedFd::from_raw_fd(SD_LISTEN_FDS_START)
        })
        .await;
        let socket = UnixListener::try_from(fd).expect("Failed to bind to ipc socket");
        let mut listener = socket.incoming();

        let count_active = Rc::new(AtomicU64::new(0));

        while let Some(stream) = listener.next().await {
            let stream = stream.expect("Failed to get next connection");
            let mut buffered_stream = BufReader::new(stream);

            let log = log.clone();
            let count_active = count_active.clone();
            let config = config.clone();
            executor
                .spawn(async move {
                    let mut current_id = None;
                    count_active.fetch_add(1, Ordering::Relaxed);
                    loop {
                        let Ok(line) = (
                            async {
                                let mut line = String::new();
                                if buffered_stream
                                    .read_line(&mut line)
                                    .await
                                    .is_ok_and(|val| val != 0)
                                {
                                    Ok(Some(line))
                                } else {
                                    if let Some(current_id) = current_id {
                                        log.lock().await.stop_event(current_id).await;
                                    }

                                    Err(())
                                }
                            },
                            async {
                                Timer::after(timeout).await;
                                Ok(None)
                            },
                        )
                            .race()
                            .await
                        else {
                            break;
                        };

                        if line.as_ref().is_none_or(|val| val.trim().is_empty()) {
                            if let Some(current_id) = current_id {
                                log.lock().await.stop_event(current_id).await;
                            }
                            current_id = None;
                        } else {
                            let line = line.unwrap(); // This was already checked
                            let line = line.trim();

                            let (language, project) =
                                line.split_once(30u8 as char).unwrap_or((line, "unknown"));

                            if !config.ignored_filetype.contains(language) {
                                current_id =
                                    Some(log.lock().await.start_event(language, project).await);
                            }
                        }
                    }
                })
                .detach();
        }
    }));

    Ok(())
}
