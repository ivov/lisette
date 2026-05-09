use std::sync::Arc;

use deps::BindgenSetup;
use tower_lsp::{LspService, Server};

use crate::workspace::WorkspaceBindgenSetup;

pub fn lsp() -> i32 {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();

        let setup: Arc<dyn BindgenSetup> = Arc::new(WorkspaceBindgenSetup);
        let (service, socket) =
            LspService::new(move |client| lsp::Backend::new(client, Some(setup.clone())));
        Server::new(stdin, stdout, socket).serve(service).await;
    });
    0
}
