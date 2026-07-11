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
        saw_shutdown: false,
        exit_sender: Some(exit_sender),
    };
    (adapter, socket, exit_receiver)
}

pub struct ProtocolAdapter<S> {
    inner: S,
    saw_shutdown: bool,
    exit_sender: Option<oneshot::Sender<i32>>,
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
            "shutdown" => self.saw_shutdown = true,
            "exit" => {
                // The LSP spec requires the process to exit with 0 when
                // shutdown preceded exit, with 1 otherwise.
                if let Some(sender) = self.exit_sender.take() {
                    let _ = sender.send(if self.saw_shutdown { 0 } else { 1 });
                }
            }
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
