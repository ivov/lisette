use crate::cli_error;

pub fn completions(shell: Option<String>) -> i32 {
    match shell.as_deref() {
        Some("bash") => {
            print!("{}", bash_completions());
            0
        }
        Some("zsh") => {
            print!("{}", zsh_completions());
            0
        }
        Some("fish") => {
            print!("{}", fish_completions());
            0
        }
        Some(other) => {
            cli_error!(
                "Unknown shell",
                format!("`{}` is not supported", other),
                "Supported shells: `bash`, `zsh`, `fish`"
            );
            1
        }
        None => {
            super::help::print_command_help("complete");
            0
        }
    }
}

fn bash_completions() -> &'static str {
    r#"_lis() {
    local cur prev commands
    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"

    commands="new run build emit check format add sync version help doc learn complete lsp"

    case "$prev" in
        lis)
            COMPREPLY=( $(compgen -W "$commands" -- "$cur") )
            return 0
            ;;
        build)
            COMPREPLY=( $(compgen -W "--sourcemap --go-flags" -- "$cur") )
            return 0
            ;;
        emit)
            COMPREPLY=( $(compgen -W "--sourcemap" -- "$cur") )
            return 0
            ;;
        run)
            COMPREPLY=( $(compgen -W "--sourcemap --go-flags" -- "$cur") )
            return 0
            ;;
        format)
            COMPREPLY=( $(compgen -W "--check" -- "$cur") )
            return 0
            ;;
        check)
            COMPREPLY=( $(compgen -W "--errors-only --warnings-only --output" -- "$cur") )
            return 0
            ;;
        --output)
            COMPREPLY=( $(compgen -W "unix" -- "$cur") )
            return 0
            ;;
        doc)
            COMPREPLY=( $(compgen -W "-s --search" -- "$cur") )
            return 0
            ;;
        complete)
            COMPREPLY=( $(compgen -W "bash zsh fish" -- "$cur") )
            return 0
            ;;
        help)
            COMPREPLY=( $(compgen -W "$commands" -- "$cur") )
            return 0
            ;;
    esac
}

complete -F _lis lis
"#
}

fn zsh_completions() -> &'static str {
    r#"#compdef lis

_lis() {
    local -a commands
    commands=(
        'new:Create a new project'
        'run:Compile and run a project'
        'build:Compile a project to Go'
        'emit:Emit Go code into target/'
        'check:Lint and typecheck a project'
        'format:Format a project'
        'add:Add a third-party Go dependency'
        'sync:Tidy project manifest'
        'version:Print compiler version'
        'help:Show help for a command'
        'doc:Browse symbols and packages'
        'learn:Create a new sample project'
        'complete:Shell completion scripts'
        'lsp:Start the language server'
    )

    _arguments -C \
        '1:command:->cmd' \
        '*::arg:->args'

    case "$state" in
        cmd)
            _describe -t commands 'lis command' commands
            ;;
        args)
            case "$words[1]" in
                build)
                    _arguments \
                        '--sourcemap[Include line directives for stack traces]' \
                        '--go-flags[Flags passed through to go build]:flags'
                    ;;
                emit)
                    _arguments '--sourcemap[Include line directives for stack traces]'
                    ;;
                run)
                    _arguments \
                        '--sourcemap[Include line directives for stack traces]' \
                        '--go-flags[Flags passed through to go build]:flags'
                    ;;
                format)
                    _arguments '--check[Check formatting without modifying]'
                    ;;
                check)
                    _arguments \
                        '--errors-only[Show only errors]' \
                        '--warnings-only[Show only warnings]' \
                        '--output[Machine-readable output]:output:(unix)'
                    ;;
                doc)
                    _arguments {-s,--search}'[Search across prelude and Go stdlib]'
                    ;;
                complete)
                    _arguments '1:shell:(bash zsh fish)'
                    ;;
                help)
                    _describe -t commands 'lis command' commands
                    ;;
            esac
            ;;
    esac
}

_lis "$@"
"#
}

fn fish_completions() -> &'static str {
    r#"complete -c lis -e
complete -c lis -f

complete -c lis -n __fish_use_subcommand -a new -d 'Create a new project'
complete -c lis -n __fish_use_subcommand -a run -d 'Compile and run a project'
complete -c lis -n __fish_use_subcommand -a build -d 'Compile a project to Go'
complete -c lis -n __fish_use_subcommand -a emit -d 'Emit Go code into target/'
complete -c lis -n __fish_use_subcommand -a check -d 'Lint and typecheck a project'
complete -c lis -n __fish_use_subcommand -a format -d 'Format a project'
complete -c lis -n __fish_use_subcommand -a add -d 'Add a third-party Go dependency'
complete -c lis -n __fish_use_subcommand -a sync -d 'Tidy project manifest'
complete -c lis -n __fish_use_subcommand -a version -d 'Print compiler version'
complete -c lis -n __fish_use_subcommand -a help -d 'Show help for a command'
complete -c lis -n __fish_use_subcommand -a doc -d 'Browse symbols and packages'
complete -c lis -n __fish_use_subcommand -a learn -d 'Create a new sample project'
complete -c lis -n __fish_use_subcommand -a complete -d 'Shell completion scripts'
complete -c lis -n __fish_use_subcommand -a lsp -d 'Start the language server'

complete -c lis -n '__fish_seen_subcommand_from build' -l sourcemap -d 'Include line directives for stack traces'
complete -c lis -n '__fish_seen_subcommand_from build' -l go-flags -r -d 'Flags passed through to go build'
complete -c lis -n '__fish_seen_subcommand_from emit' -l sourcemap -d 'Include line directives for stack traces'
complete -c lis -n '__fish_seen_subcommand_from run' -l sourcemap -d 'Include line directives for stack traces'
complete -c lis -n '__fish_seen_subcommand_from run' -l go-flags -r -d 'Flags passed through to go build'
complete -c lis -n '__fish_seen_subcommand_from format' -l check -d 'Check formatting without modifying'
complete -c lis -n '__fish_seen_subcommand_from check' -l errors-only -d 'Show only errors'
complete -c lis -n '__fish_seen_subcommand_from check' -l warnings-only -d 'Show only warnings'
complete -c lis -n '__fish_seen_subcommand_from check' -l output -r -a unix -d 'Machine-readable output'
complete -c lis -n '__fish_seen_subcommand_from doc' -s s -l search -d 'Search across prelude and Go stdlib'
complete -c lis -n '__fish_seen_subcommand_from complete' -a 'bash zsh fish' -d 'Shell type'
complete -c lis -n '__fish_seen_subcommand_from help' -a 'new run build emit check format add sync version help doc learn complete lsp' -d 'Command'
"#
}
