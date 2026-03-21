use tower_lsp::{LspService, Server};

pub fn lsp() -> i32 {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();

        let (service, socket) = LspService::new(lsp::Backend::new);
        Server::new(stdin, stdout, socket).serve(service).await;
    });
    0
}
