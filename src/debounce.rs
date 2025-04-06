use std::time::Duration;

use futures_concurrency::future::Race;
use smol::{stream::StreamExt, LocalExecutor, Timer};

pub fn debounce<T: 'static, const S: usize>(
    time: Duration,
    mut receiver: crate::Receiver<T, S>,
    executor: &LocalExecutor<'_>,
) -> crate::Receiver<T, S> {
    let (output_sender, output_receiver) = crate::channel();

    executor
        .spawn(async move {
            loop {
                let Some(mut value) = receiver.next().await else {
                    return;
                };

                loop {
                    let new_value = (
                        async {
                            Timer::after(time).await;
                            None
                        },
                        async { Some(receiver.next().await) },
                    )
                        .race()
                        .await;

                    match new_value {
                        Some(Some(new_value)) => value = new_value,
                        Some(None) => return,
                        None => break,
                    }
                }

                output_sender.send(value)
            }
        })
        .detach();

    output_receiver
}
