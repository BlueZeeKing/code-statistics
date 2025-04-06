use std::{
    cell::Cell,
    future::poll_fn,
    io::ErrorKind,
    os::fd::{FromRawFd, OwnedFd},
    ptr,
    rc::Rc,
    task::Poll,
};

use chrono::Utc;
use code_statistics::{
    config::{read_config, Config},
    log::{log, LogMessage},
    sd_is_socket_unix, sd_listen_fds,
    tags::Tags,
    SD_LISTEN_FDS_START,
};
use dirs::data_dir;
use futures_concurrency::future::Race;
use smol::{
    fs::create_dir_all,
    io::{AsyncBufReadExt, BufReader},
    net::unix::UnixListener,
    stream::StreamExt,
    unblock, Timer,
};
use tracing::{debug, info, trace, warn};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

fn main() -> Result<(), ()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

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

        let config: &'static Config = Box::leak(Box::new(read_config().await));

        debug!(?config);

        let log = log(&executor, config);

        let languages = Rc::new(Tags::new("languages").await);
        let projects = Rc::new(Tags::new("projects").await);

        let fd = unblock(|| {
            let num_descriptors = unsafe { sd_listen_fds(1) };
            if num_descriptors <= 0 {
                warn!("Failed to get sockets from systemd");
                return None;
            }
            if unsafe { sd_is_socket_unix(SD_LISTEN_FDS_START, 1, 1, ptr::null(), 0) } != 1 {
                warn!("Wrong kind of socket");
                return None;
            }
            Some(unsafe { OwnedFd::from_raw_fd(SD_LISTEN_FDS_START) })
        })
        .await;
        let socket = if let Some(fd) = fd {
            UnixListener::try_from(fd).expect("Failed to bind to ipc socket")
        } else {
            warn!("Falling back to non systemd socket");
            UnixListener::bind("/run/user/1000/code-statistics")
                .expect("Failed to bind to ipc socket")
        };
        let mut listener = socket.incoming();

        let ids = Rc::new(Cell::new(0));

        while let Some(stream) = listener.next().await {
            let stream = stream.expect("Failed to get next connection");
            let mut buffered_stream = BufReader::new(stream);

            let log = log.clone();
            let languages = languages.clone();
            let projects = projects.clone();

            let id = ids.get();
            ids.set(id + 1);

            info!("Client joined with id {}", id);

            executor
                .spawn(async move {
                    let mut last_message = None;

                    loop {
                        let Ok(line): Result<String, std::io::Error> = (
                            async {
                                let mut line = String::new();
                                let amount_read = buffered_stream.read_line(&mut line).await?;
                                if amount_read == 0 {
                                    Err(std::io::Error::new(
                                        ErrorKind::NotConnected,
                                        "Client no longer connected",
                                    ))
                                } else {
                                    Ok(line)
                                }
                            },
                            async {
                                if last_message.is_none() {
                                    poll_fn(|_| Poll::<()>::Pending).await;
                                } else {
                                    Timer::after(config.timeout).await;
                                }
                                Ok("".to_string())
                            },
                        )
                            .race()
                            .await
                        else {
                            debug!("Client disconnected");
                            log.send(LogMessage::End {
                                id,
                                time: Utc::now(),
                            });
                            break;
                        };

                        if line.trim().is_empty() {
                            debug!("Received end");
                            log.send(LogMessage::End {
                                id,
                                time: Utc::now(),
                            });
                            last_message = None;
                        } else {
                            let line = line.trim();

                            debug!("Received line: {}", line);

                            let (language, project) =
                                line.split_once(30u8 as char).unwrap_or((line, "unknown"));

                            debug!(language, project);

                            if config.ignored_languages.contains(language) {
                                trace!("skipping ignored language");
                                continue;
                            }

                            if language.is_empty() {
                                trace!("ignoring empty language");
                                continue;
                            }

                            let language = languages.get(language).await;
                            let project = projects.get(project).await;

                            if last_message == Some((language, project)) {
                                trace!("Skipping sending same language and project");
                                continue;
                            }

                            last_message = Some((language, project));

                            log.send(LogMessage::Start {
                                id,
                                time: Utc::now(),
                                language,
                                project,
                            });
                        }
                    }
                })
                .detach();
        }
    }));

    Ok(())
}
