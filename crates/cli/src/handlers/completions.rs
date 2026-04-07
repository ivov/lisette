use crate::output::print_help;

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
            eprintln!("unknown shell: {}", other);
            eprintln!("supported shells: bash, zsh, fish");
            1
        }
        None => {
            print_help(
                "`lis completions` <shell>

Generate shell completion scripts.

Arguments:
    <shell>    Shell to generate completions for (`bash`, `zsh`, or `fish`)

Usage:
    `lis completions bash` > ~/.local/share/bash-completion/completions/lis
    `lis completions zsh`  > ~/.zfunc/_lis
    `lis completions fish` > ~/.config/fish/completions/lis.fish",
            );
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

    commands="new build run format check clean help version add remove list lsp learn doc bindgen completions"

    case "$prev" in
        lis)
            COMPREPLY=( $(compgen -W "$commands" -- "$cur") )
            return 0
            ;;
        build|b)
            COMPREPLY=( $(compgen -W "--debug" -- "$cur") )
            return 0
            ;;
        run|r)
            COMPREPLY=( $(compgen -W "--debug --" -- "$cur") )
            return 0
            ;;
        format|f)
            COMPREPLY=( $(compgen -W "--check" -- "$cur") )
            return 0
            ;;
        check|c)
            COMPREPLY=( $(compgen -W "--errors-only --warnings-only" -- "$cur") )
            return 0
            ;;
        bindgen)
            COMPREPLY=( $(compgen -W "-o --output -v --verbose" -- "$cur") )
            return 0
            ;;
        doc)
            COMPREPLY=( $(compgen -W "-s --search" -- "$cur") )
            return 0
            ;;
        completions)
            COMPREPLY=( $(compgen -W "bash zsh fish" -- "$cur") )
            return 0
            ;;
        help)
            COMPREPLY=( $(compgen -W "$commands" -- "$cur") )
            return 0
            ;;
    esac

    COMPREPLY=( $(compgen -W "$commands" -- "$cur") )
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
        'build:Compile a project'
        'run:Compile and run a project'
        'format:Format a file or project'
        'check:Validate a file or project'
        'clean:Remove build artifacts'
        'help:Print help message'
        'version:Print version'
        'add:Add a dependency'
        'remove:Remove a dependency'
        'list:List dependencies'
        'lsp:Start language server'
        'learn:Generate a sample project'
        'doc:Explore prelude and Go stdlib'
        'bindgen:Generate type definition bindings'
        'completions:Generate shell completions'
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
                build|b)
                    _arguments \
                        '--debug[Include line directives for stack traces]' \
                        '1:path:_files -/'
                    ;;
                run|r)
                    _arguments \
                        '--debug[Include line directives for stack traces]' \
                        '1:target:_files'
                    ;;
                format|f)
                    _arguments \
                        '--check[Check formatting without modifying]' \
                        '1:path:_files'
                    ;;
                check|c)
                    _arguments \
                        '--errors-only[Show only errors]' \
                        '--warnings-only[Show only warnings]' \
                        '1:path:_files'
                    ;;
                clean|x)
                    _arguments '1:path:_files -/'
                    ;;
                new)
                    _arguments '1:name:'
                    ;;
                add)
                    _arguments '1:dependency:'
                    ;;
                remove)
                    _arguments '1:dependency:'
                    ;;
                bindgen)
                    _arguments \
                        {-o,--output}'=[Output file path]:path:_files' \
                        {-v,--verbose}'[Show verbose output]' \
                        '1:package:'
                    ;;
                doc)
                    _arguments \
                        {-s,--search}'[Search across prelude and Go stdlib]' \
                        '1:query:'
                    ;;
                completions)
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

complete -c lis -n __fish_use_subcommand -a new -d 'Create a new project'
complete -c lis -n __fish_use_subcommand -a build -d 'Compile a project'
complete -c lis -n __fish_use_subcommand -a run -d 'Compile and run a project'
complete -c lis -n __fish_use_subcommand -a format -d 'Format a file or project'
complete -c lis -n __fish_use_subcommand -a check -d 'Validate a file or project'
complete -c lis -n __fish_use_subcommand -a clean -d 'Remove build artifacts'
complete -c lis -n __fish_use_subcommand -a help -d 'Print help message'
complete -c lis -n __fish_use_subcommand -a version -d 'Print version'
complete -c lis -n __fish_use_subcommand -a add -d 'Add a dependency'
complete -c lis -n __fish_use_subcommand -a remove -d 'Remove a dependency'
complete -c lis -n __fish_use_subcommand -a list -d 'List dependencies'
complete -c lis -n __fish_use_subcommand -a lsp -d 'Start language server'
complete -c lis -n __fish_use_subcommand -a learn -d 'Generate a sample project'
complete -c lis -n __fish_use_subcommand -a doc -d 'Explore prelude and Go stdlib'
complete -c lis -n __fish_use_subcommand -a bindgen -d 'Generate type definition bindings'
complete -c lis -n __fish_use_subcommand -a completions -d 'Generate shell completions'

complete -c lis -n '__fish_seen_subcommand_from build' -l debug -d 'Include line directives for stack traces'
complete -c lis -n '__fish_seen_subcommand_from run' -l debug -d 'Include line directives for stack traces'
complete -c lis -n '__fish_seen_subcommand_from format' -l check -d 'Check formatting without modifying'
complete -c lis -n '__fish_seen_subcommand_from check' -l errors-only -d 'Show only errors'
complete -c lis -n '__fish_seen_subcommand_from check' -l warnings-only -d 'Show only warnings'
complete -c lis -n '__fish_seen_subcommand_from bindgen' -s o -l output -d 'Output file path' -r
complete -c lis -n '__fish_seen_subcommand_from bindgen' -s v -l verbose -d 'Show verbose output'
complete -c lis -n '__fish_seen_subcommand_from doc' -s s -l search -d 'Search across prelude and Go stdlib'
complete -c lis -n '__fish_seen_subcommand_from completions' -a 'bash zsh fish' -d 'Shell type'
complete -c lis -n '__fish_seen_subcommand_from help' -a 'new build run format check clean help version add remove list lsp learn doc bindgen completions' -d 'Command'
"#
}
