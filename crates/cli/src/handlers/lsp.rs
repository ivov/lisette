use std::sync::Arc;

use deps::BindgenSetup;

use crate::workspace::WorkspaceBindgenSetup;
use lsp::protocol;
use std::io;

pub fn lsp() -> i32 {
    let setup: Arc<dyn BindgenSetup> = Arc::new(WorkspaceBindgenSetup);
    protocol::serve(io::stdin().lock(), io::stdout(), Some(setup))
}
