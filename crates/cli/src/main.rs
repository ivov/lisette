mod agents_md;
mod command;
mod go_cli;
mod handlers;
mod output;
mod panic;

use command::Command;

fn main() {
    panic::add_handler();

    let args: Vec<String> = std::env::args().collect();

    let command = match Command::parse(args) {
        Ok(command) => command,
        Err(command::ParseError::MissingArgument { command, argument }) => {
            cli_error!(
                "Missing argument",
                format!("`lis {}` requires `<{}>` argument", command, argument),
                format!("Run `lis help {}` for usage", command)
            );
            std::process::exit(1);
        }
        Err(command::ParseError::UnknownCommand(cmd)) => {
            let hint = match command::Command::suggest(&cmd) {
                Some(suggestion) => format!("Did you mean `{}`?", suggestion),
                None => "Run `lis help` for available commands".to_string(),
            };
            cli_error!(
                "Unknown command",
                format!("`{}` is not a lis command", cmd),
                hint
            );
            std::process::exit(1);
        }
        Err(command::ParseError::UnknownFlag(flag)) => {
            cli_error!(
                "Unknown flag",
                format!("`{}` is not a valid flag", flag),
                "Run `lis help` for available flags"
            );
            std::process::exit(1);
        }
        Err(command::ParseError::UnexpectedArgument { message, hint }) => {
            cli_error!(
                message,
                "The `doc` command takes a single query argument",
                hint
            );
            std::process::exit(1);
        }
    };

    let exit_code = match command {
        Command::New { name } => handlers::new_project(&name),
        Command::Build { path, debug } => handlers::build(path, debug, false),
        Command::Run {
            target,
            args,
            debug,
        } => handlers::run(target, args, debug),
        Command::Format { path, check } => handlers::format(path, check),
        Command::Check {
            path,
            errors_only,
            warnings_only,
        } => handlers::check(path, errors_only, warnings_only),
        Command::Clean { path } => handlers::clean(path),
        Command::Help { command } => {
            match command {
                Some(cmd) => handlers::help::print_command_help(&cmd),
                None => handlers::help::print_main_help(),
            }
            0
        }
        Command::Version => {
            handlers::help::print_version();
            0
        }
        Command::Add { dependency } => handlers::add(&dependency),
        Command::Remove { dependency } => handlers::remove(&dependency),
        Command::List => handlers::list(),
        Command::Lsp => handlers::lsp(),
        Command::Bindgen {
            package,
            output,
            version,
            verbose,
        } => handlers::bindgen(&package, output, version, verbose),
        Command::Doc { query } => handlers::doc(query),
        Command::DocSearch { query } => handlers::doc_search(&query),
        Command::Learn => handlers::learn(),
        Command::Completions { shell } => handlers::completions(shell),
    };

    std::process::exit(exit_code);
}
