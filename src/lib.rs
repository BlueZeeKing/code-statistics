#![feature(c_size_t, local_waker)]

use core::ffi::{c_char, c_int, c_size_t};
use std::{
    cell::RefCell,
    future::poll_fn,
    pin::Pin,
    rc::{Rc, Weak},
    task::{Context, LocalWaker, Poll},
};

use smol::stream::Stream;

pub mod config;
pub mod debounce;
pub mod log;
pub mod manager;
pub mod tags;

pub const SD_LISTEN_FDS_START: i32 = 3;

#[link(name = "systemd")]
extern "C" {
    /// Returns how many file descriptors have been passed, or a negative
    /// errno code on failure. Optionally, removes the $LISTEN_FDS and
    /// $LISTEN_PID file descriptors from the environment (recommended, but
    /// problematic in threaded environments). If r is the return value of
    /// this function you'll find the file descriptors passed as fds
    /// SD_LISTEN_FDS_START to SD_LISTEN_FDS_START+r-1. Returns a negative
    /// errno style error code on failure. This function call ensures that
    /// the FD_CLOEXEC flag is set for the passed file descriptors, to make
    /// sure they are not passed on to child processes. If FD_CLOEXEC shall
    /// not be set, the caller needs to unset it after this call for all file
    /// descriptors that are used.
    ///
    /// See sd_listen_fds(3) for more information.
    pub fn sd_listen_fds(unset_environment: c_int) -> c_int;

    /// Helper call for identifying a passed file descriptor. Returns 1 if
    /// the file descriptor is an AF_UNIX socket of the specified type
    /// (SOCK_DGRAM, SOCK_STREAM, ...) and path, 0 otherwise. If type is 0
    /// a socket type check will not be done. If path is NULL a socket path
    /// check will not be done. For normal AF_UNIX sockets set length to
    /// 0. For abstract namespace sockets set length to the length of the
    /// socket name (including the initial 0 byte), and pass the full
    /// socket path in path (including the initial 0 byte). The listening
    /// flag is used the same way as in sd_is_socket(). Returns a negative
    /// errno style error code on failure.
    ///
    /// See sd_is_socket_unix(3) for more information.
    pub fn sd_is_socket_unix(
        fd: c_int,
        r#type: c_int,
        listening: c_int,
        path: *const c_char,
        length: c_size_t,
    ) -> c_int;
}

struct ChannelState<T, const S: usize> {
    data: [Option<T>; S],
    read_idx: usize,
    write_idx: usize,
    waker: Option<LocalWaker>,
}

pub struct Receiver<T, const S: usize> {
    state: Rc<RefCell<ChannelState<T, S>>>,
}

impl<T, const S: usize> Receiver<T, S> {
    fn poll_next_inner(&self, cx: &mut Context<'_>) -> Poll<Option<T>> {
        if Rc::weak_count(&self.state) == 0 {
            return Poll::Ready(None);
        }
        let mut state = self.state.borrow_mut();

        if state.read_idx != state.write_idx {
            let idx = state.read_idx;
            let item = state.data[idx].take();
            state.read_idx += 1;
            state.read_idx %= S;
            Poll::Ready(Some(item.unwrap()))
        } else {
            state.waker = Some(cx.local_waker().to_owned());
            Poll::Pending
        }
    }

    pub async fn recv(&self) -> Option<T> {
        poll_fn(|cx| self.poll_next_inner(cx)).await
    }
}

impl<T, const S: usize> Stream for Receiver<T, S> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::into_inner(self).poll_next_inner(cx)
    }
}

pub struct Sender<T, const S: usize> {
    state: Weak<RefCell<ChannelState<T, S>>>,
}

impl<T, const S: usize> Clone for Sender<T, S> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl<T, const S: usize> Sender<T, S> {
    pub fn send(&self, value: T) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let mut state = state.borrow_mut();
        let idx = state.write_idx;
        state.data[idx] = Some(value);
        state.write_idx += 1;
        state.write_idx %= S;
        let Some(waker) = state.waker.take() else {
            return;
        };
        drop(state);
        waker.wake();
    }
}

pub fn channel<T, const S: usize>() -> (Sender<T, S>, Receiver<T, S>) {
    let state = Rc::new(RefCell::new(ChannelState {
        data: [const { None }; S],
        read_idx: 0,
        write_idx: 0,
        waker: None,
    }));

    let weak_state = Rc::downgrade(&state);

    (Sender { state: weak_state }, Receiver { state })
}
