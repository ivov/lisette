//! Bounded semantic workers, cancellable requests, and coalesced diagnostics.

use std::collections::{HashMap, VecDeque};
use std::mem;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::{Error, MessageType, RpcResult, dispatch};
use crate::state::{AnalysisKey, Backend, CancellationToken};

#[derive(Clone, Debug, Hash, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum RequestId {
    Number(i64),
    String(String),
}

struct Request {
    id: RequestId,
    method: String,
    params: Value,
    generation: u64,
    token: CancellationToken,
}

struct Diagnostics {
    deadline: Instant,
    token: CancellationToken,
}

enum Work {
    Request(Request),
    Diagnostics(AnalysisKey, CancellationToken),
}

#[derive(Default)]
struct Queue {
    stopped: bool,
    requests: VecDeque<Request>,
    pending: HashMap<RequestId, CancellationToken>,
    diagnostics: HashMap<AnalysisKey, Diagnostics>,
    prefer_diagnostics: bool,
}

#[derive(Default)]
pub(crate) struct Scheduler {
    queue: Mutex<Queue>,
    wake: Condvar,
}

impl Scheduler {
    pub(crate) fn start(self: &Arc<Self>, backend: &Backend) -> Vec<JoinHandle<()>> {
        (0..2)
            .map(|_| {
                let scheduler = Arc::clone(self);
                let backend = Backend {
                    shared_state: Arc::clone(&backend.shared_state),
                };
                thread::spawn(move || scheduler.run(&backend))
            })
            .collect()
    }

    pub(crate) fn request(&self, id: RequestId, method: String, params: Value, generation: u64) {
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        // A duplicate active ID cannot identify a second response or cancellation.
        if queue.stopped || queue.pending.contains_key(&id) {
            return;
        }
        let token = CancellationToken::new();
        queue.pending.insert(id.clone(), token.clone());
        queue.requests.push_back(Request {
            id,
            method,
            params,
            generation,
            token,
        });
        self.wake.notify_one();
    }

    pub(crate) fn cancel(&self, backend: &Backend, id: RequestId) {
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(token) = queue.pending.remove(&id) {
            token.cancel();
            queue.requests.retain(|request| request.id != id);
            backend.client.respond(id, Err(Error::cancelled()));
        }
    }

    pub(crate) fn diagnostics(&self, key: AnalysisKey, token: CancellationToken, delay: Duration) {
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        if queue.stopped {
            token.cancel();
            return;
        }
        queue
            .diagnostics
            .retain(|_, scheduled| !scheduled.token.is_cancelled());
        queue.diagnostics.insert(
            key,
            Diagnostics {
                deadline: Instant::now() + delay,
                token,
            },
        );
        self.wake.notify_all();
    }

    pub(crate) fn stop(&self, backend: &Backend) {
        // Match the workspace-before-queue order used when finishing requests.
        backend.workspace_mut().cancel_diagnostics();
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        queue.stopped = true;
        for (id, token) in queue.pending.drain() {
            token.cancel();
            backend.client.respond(id, Err(Error::cancelled()));
        }
        queue.requests.clear();
        queue.diagnostics.clear();
        self.wake.notify_all();
    }

    fn next_work(&self) -> Option<Work> {
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if queue.stopped {
                return None;
            }
            queue
                .diagnostics
                .retain(|_, scheduled| !scheduled.token.is_cancelled());
            let next = queue
                .diagnostics
                .iter()
                .min_by_key(|(_, scheduled)| scheduled.deadline)
                .map(|(key, scheduled)| (key.clone(), scheduled.deadline));
            let due = next
                .as_ref()
                .is_some_and(|(_, deadline)| *deadline <= Instant::now());
            // Alternate when both kinds are ready so requests cannot starve diagnostics.
            if !queue.requests.is_empty() && (!due || !queue.prefer_diagnostics) {
                queue.prefer_diagnostics = true;
                return queue.requests.pop_front().map(Work::Request);
            }
            if due && let Some((key, _)) = next {
                queue.prefer_diagnostics = false;
                if let Some(scheduled) = queue.diagnostics.remove(&key) {
                    return Some(Work::Diagnostics(key, scheduled.token));
                }
            } else if let Some((_, deadline)) = next {
                (queue, _) = self
                    .wake
                    .wait_timeout(queue, deadline.saturating_duration_since(Instant::now()))
                    .unwrap_or_else(PoisonError::into_inner);
            } else {
                queue = self
                    .wake
                    .wait(queue)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        }
    }

    fn run(&self, backend: &Backend) {
        while let Some(work) = self.next_work() {
            match work {
                Work::Request(mut request) => {
                    if request.token.is_cancelled() {
                        continue;
                    }
                    let response = if backend.workspace().generation() != request.generation {
                        Err(Error::content_modified())
                    } else {
                        catch_unwind(AssertUnwindSafe(|| {
                            dispatch(backend, &request.method, mem::take(&mut request.params))
                        }))
                        .unwrap_or_else(|_| Err(Error::internal("Request handler panicked")))
                    };
                    self.finish(backend, request, response);
                }
                Work::Diagnostics(key, token) => {
                    let result =
                        catch_unwind(AssertUnwindSafe(|| backend.run_diagnostics(&key, &token)));
                    if result.is_err() {
                        backend
                            .client
                            .log_message(MessageType::WARNING, "Diagnostics handler panicked");
                    }
                    backend.workspace_mut().finish_diagnostics(&key, &token);
                }
            }
        }
    }

    fn finish(&self, backend: &Backend, request: Request, response: RpcResult<Value>) {
        let workspace = backend.workspace();
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        // ID reuse must not let an old worker complete a newer request.
        if queue.pending.get(&request.id) != Some(&request.token) {
            return;
        }
        queue.pending.remove(&request.id);
        let response = if workspace.generation() != request.generation {
            Err(Error::content_modified())
        } else {
            response
        };
        backend.client.respond(request.id, response);
    }
}
