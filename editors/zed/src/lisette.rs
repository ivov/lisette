use zed_extension_api::{self as zed, settings::LspSettings, LanguageServerId, Result};

struct LisetteExtension;

const DEFAULT_ARGS: &[&str] = &["lsp"];

impl zed::Extension for LisetteExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let (command, args) = match LspSettings::for_worktree("lisette-lsp", worktree) {
            Ok(settings) => match settings.binary.and_then(|b| b.path.map(|p| (p, b.arguments))) {
                Some((path, args)) => (
                    path,
                    args.unwrap_or_else(|| DEFAULT_ARGS.iter().map(|s| s.to_string()).collect()),
                ),
                None => (self.find_binary(worktree)?, default_args()),
            },
            Err(_) => (self.find_binary(worktree)?, default_args()),
        };

        Ok(zed::Command {
            command,
            args,
            env: worktree.shell_env(),
        })
    }
}

impl LisetteExtension {
    fn find_binary(&self, worktree: &zed::Worktree) -> Result<String> {
        worktree.which("lis").ok_or_else(|| {
            "Could not find `lis` binary in PATH. Install Lisette or configure \
             lsp.lisette-lsp.binary.path in Zed settings."
                .to_string()
        })
    }
}

fn default_args() -> Vec<String> {
    DEFAULT_ARGS.iter().map(|s| s.to_string()).collect()
}

zed::register_extension!(LisetteExtension);
