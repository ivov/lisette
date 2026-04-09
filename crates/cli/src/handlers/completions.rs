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
    `lis completions fish` > ~/.config/fish/completions/lis.fish

    For zsh, add to ~/.zshrc (before compinit):
        fpath=(~/.zfunc $fpath)
    Then generate:
        mkdir -p ~/.zfunc && `lis completions zsh` > ~/.zfunc/_lis",
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

    commands="new build run format check clean learn doc help completions"

    case "$prev" in
        lis)
            COMPREPLY=( $(compgen -W "$commands" -- "$cur") )
            return 0
            ;;
        build)
            COMPREPLY=( $(compgen -W "--debug" -- "$cur") )
            return 0
            ;;
        run)
            COMPREPLY=( $(compgen -W "--debug" -- "$cur") )
            return 0
            ;;
        format)
            COMPREPLY=( $(compgen -W "--check" -- "$cur") )
            return 0
            ;;
        check)
            COMPREPLY=( $(compgen -W "--errors-only --warnings-only" -- "$cur") )
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
        'learn:Generate a sample project'
        'doc:Explore prelude and Go stdlib'
        'help:Print help message'
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
                build)
                    _arguments '--debug[Include line directives for stack traces]'
                    ;;
                run)
                    _arguments '--debug[Include line directives for stack traces]'
                    ;;
                format)
                    _arguments '--check[Check formatting without modifying]'
                    ;;
                check)
                    _arguments \
                        '--errors-only[Show only errors]' \
                        '--warnings-only[Show only warnings]'
                    ;;
                doc)
                    _arguments {-s,--search}'[Search across prelude and Go stdlib]'
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
complete -c lis -f

complete -c lis -n __fish_use_subcommand -a new -d 'Create a new project'
complete -c lis -n __fish_use_subcommand -a build -d 'Compile a project'
complete -c lis -n __fish_use_subcommand -a run -d 'Compile and run a project'
complete -c lis -n __fish_use_subcommand -a format -d 'Format a file or project'
complete -c lis -n __fish_use_subcommand -a check -d 'Validate a file or project'
complete -c lis -n __fish_use_subcommand -a clean -d 'Remove build artifacts'
complete -c lis -n __fish_use_subcommand -a learn -d 'Generate a sample project'
complete -c lis -n __fish_use_subcommand -a doc -d 'Explore prelude and Go stdlib'
complete -c lis -n __fish_use_subcommand -a help -d 'Print help message'
complete -c lis -n __fish_use_subcommand -a completions -d 'Generate shell completions'

complete -c lis -n '__fish_seen_subcommand_from build' -l debug -d 'Include line directives for stack traces'
complete -c lis -n '__fish_seen_subcommand_from run' -l debug -d 'Include line directives for stack traces'
complete -c lis -n '__fish_seen_subcommand_from format' -l check -d 'Check formatting without modifying'
complete -c lis -n '__fish_seen_subcommand_from check' -l errors-only -d 'Show only errors'
complete -c lis -n '__fish_seen_subcommand_from check' -l warnings-only -d 'Show only warnings'
complete -c lis -n '__fish_seen_subcommand_from doc' -s s -l search -d 'Search across prelude and Go stdlib'
complete -c lis -n '__fish_seen_subcommand_from completions' -a 'bash zsh fish' -d 'Shell type'
complete -c lis -n '__fish_seen_subcommand_from help' -a 'new build run format check clean learn doc help completions' -d 'Command'
"#
}
