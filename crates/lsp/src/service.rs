use std::sync::Arc;
use std::task::{Context, Poll};

use deps::BindgenSetup;
use tokio::sync::oneshot;
use tower::Service;
use tower_lsp::jsonrpc::Request;
use tower_lsp::{ClientSocket, LspService};

use crate::state::Backend;

pub fn build_service(
    bindgen_setup: Option<Arc<dyn BindgenSetup>>,
) -> (
    ProtocolAdapter<LspService<Backend>>,
    ClientSocket,
    oneshot::Receiver<i32>,
) {
    let (service, socket) =
        LspService::new(move |client| Backend::new(client, bindgen_setup.clone()));
    let (exit_sender, exit_receiver) = oneshot::channel();
    let adapter = ProtocolAdapter {
        inner: service,
        state: ProtocolState::Running(exit_sender),
    };
    (adapter, socket, exit_receiver)
}

pub struct ProtocolAdapter<S> {
    inner: S,
    state: ProtocolState,
}

enum ProtocolState {
    Running(oneshot::Sender<i32>),
    Shutdown(oneshot::Sender<i32>),
    Exited,
}

impl ProtocolState {
    fn shutdown(&mut self) {
        let current = std::mem::replace(self, Self::Exited);
        *self = match current {
            Self::Running(sender) | Self::Shutdown(sender) => Self::Shutdown(sender),
            Self::Exited => Self::Exited,
        };
    }

    fn exit(&mut self) {
        let current = std::mem::replace(self, Self::Exited);
        let (sender, code) = match current {
            Self::Running(sender) => (sender, 1),
            Self::Shutdown(sender) => (sender, 0),
            Self::Exited => return,
        };
        let _ = sender.send(code);
    }
}

impl<S: Service<Request>> Service<Request> for ProtocolAdapter<S> {
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(context)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        match request.method() {
            "shutdown" => self.state.shutdown(),
            "exit" => self.state.exit(),
            _ => {}
        }
        let request = if request.params().is_some_and(|params| params.is_null()) {
            let (method, id, _) = request.into_parts();
            let builder = Request::build(method);
            match id {
                Some(id) => builder.id(id).finish(),
                None => builder.finish(),
            }
        } else {
            request
        };
        self.inner.call(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_after_shutdown_reports_success() {
        let (sender, mut receiver) = oneshot::channel();
        let mut state = ProtocolState::Running(sender);

        state.shutdown();
        state.exit();

        assert_eq!(receiver.try_recv(), Ok(0));
    }

    #[test]
    fn exit_without_shutdown_reports_failure() {
        let (sender, mut receiver) = oneshot::channel();
        let mut state = ProtocolState::Running(sender);

        state.exit();

        assert_eq!(receiver.try_recv(), Ok(1));
    }

    #[test]
    fn repeated_exit_keeps_first_result() {
        let (sender, mut receiver) = oneshot::channel();
        let mut state = ProtocolState::Running(sender);

        state.exit();
        state.exit();

        assert_eq!(receiver.try_recv(), Ok(1));
    }
}
