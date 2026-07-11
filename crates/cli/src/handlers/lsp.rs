use std::sync::Arc;

use deps::BindgenSetup;
use tower_lsp::Server;

use crate::workspace::WorkspaceBindgenSetup;

pub fn lsp() -> i32 {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let code = rt.block_on(async {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();

        let setup: Arc<dyn BindgenSetup> = Arc::new(WorkspaceBindgenSetup);
        let (service, socket, exited) = lsp::build_service(Some(setup));
        tokio::select! {
            _ = Server::new(stdin, stdout, socket).serve(service) => 0,
            code = exited => code.unwrap_or(0),
        }
    });
    // Dropping the runtime would block on the stdin reader thread, which
    // stays stuck in a read while the client keeps the pipe open.
    rt.shutdown_background();
    code
}
