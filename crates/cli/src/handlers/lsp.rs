use std::sync::Arc;

use deps::BindgenSetup;

use crate::workspace::WorkspaceBindgenSetup;

pub fn lsp() -> i32 {
    let setup: Arc<dyn BindgenSetup> = Arc::new(WorkspaceBindgenSetup);
    lsp::protocol::serve(std::io::stdin().lock(), std::io::stdout(), Some(setup))
}
