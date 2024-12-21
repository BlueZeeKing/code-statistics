#![feature(c_size_t)]

use core::ffi::{c_char, c_int, c_size_t};

pub mod config;
pub mod heartbeat;
pub mod log;
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
