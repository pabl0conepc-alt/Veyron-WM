//! Control socket bridge between the IPC threads and the compositor loop.
//!
//! The compositor is single threaded: all window-management state lives in the
//! event loop. IPC connections are therefore handled on their own threads and
//! forwarded as [`Job`]s through a `calloop` channel; the reply travels back on
//! a oneshot channel so the client always gets an answer.

use std::{sync::mpsc, time::Duration};

use calloop::channel::Sender;
use veyron_ipc::{Reply, Request, Response};

/// How long a client waits for the compositor to answer before giving up.
const REPLY_TIMEOUT: Duration = Duration::from_secs(2);

/// A request plus the channel its answer must be returned on.
#[derive(Debug)]
pub struct Job {
    pub request: Request,
    pub reply: mpsc::Sender<Response>,
}

/// Handler given to the IPC server: forwards the request and waits for the
/// reply. A subscription instead hands the reply channel to the compositor,
/// which keeps sending events on it.
pub fn handle(sender: &Sender<Job>, request: Request) -> Reply {
    let subscribe = request == Request::Subscribe;
    let (reply, receiver) = mpsc::channel();
    if sender.send(Job { request, reply }).is_err() {
        return Response::Error {
            message: "the compositor is shutting down".to_string(),
        }
        .into();
    }
    if subscribe {
        return Reply::Stream(receiver);
    }

    match receiver.recv_timeout(REPLY_TIMEOUT) {
        Ok(response) => response,
        Err(_) => Response::Error {
            message: "the compositor did not answer in time".to_string(),
        },
    }
    .into()
}
