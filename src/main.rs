use std::{rc::Rc, time::Duration};

use code_statistics::{heartbeat::start_heartbeat, log::Log, tags::Tags};
use dirs::{data_dir, runtime_dir};
use futures_concurrency::future::Race;
use smol::{
    fs::create_dir_all,
    io::{AsyncBufReadExt, BufReader},
    lock::Mutex,
    net::unix::UnixListener,
    stream::StreamExt,
    Timer,
};

fn main() {
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

        let initial_heartbeat = start_heartbeat(&executor).await;
        let log = Rc::new(Mutex::new(
            Log::new(
                initial_heartbeat,
                Tags::new("languages").await,
                Tags::new("projects").await,
            )
            .await,
        ));

        let mut socket_path = runtime_dir().expect("Failed to get runtime directory");
        socket_path.push("code-statistics");

        let socket = UnixListener::bind(socket_path).expect("Failed to bind to ipc socket");

        let mut listener = socket.incoming();

        while let Some(stream) = listener.next().await {
            let stream = stream.expect("Failed to get next connection");
            let mut buffered_stream = BufReader::new(stream);

            let log = log.clone();
            executor
                .spawn(async move {
                    let mut current_id = None;
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
                                Timer::after(Duration::from_secs(60)).await;
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

                            current_id =
                                Some(log.lock().await.start_event(language, project).await);
                        }
                    }
                })
                .detach();
        }
    }));
}
