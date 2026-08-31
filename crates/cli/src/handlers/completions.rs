use crate::cli_error;

pub fn completions(shell: Option<String>) -> i32 {
    match shell.as_deref() {
        Some("bash") => {
            print!("{}", with_targets(bash_completions()));
            0
        }
        Some("zsh") => {
            print!("{}", with_targets(zsh_completions()));
            0
        }
        Some("fish") => {
            print!("{}", with_targets(fish_completions()));
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

fn with_targets(script: &str) -> String {
    let words = stdlib::SUPPORTED_TARGETS
        .iter()
        .map(stdlib::Target::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    script.replace("@TARGETS@", &words)
}

fn bash_completions() -> &'static str {
    r#"_lis() {
    local cur prev commands
    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"

    commands="new run build emit check format test add sync version upgrade help doc learn complete lsp"

    case "$prev" in
        lis)
            COMPREPLY=( $(compgen -W "$commands" -- "$cur") )
            return 0
            ;;
        build)
            COMPREPLY=( $(compgen -W "--sourcemap --go-flags -o --output --target" -- "$cur") )
            return 0
            ;;
        emit)
            COMPREPLY=( $(compgen -W "--sourcemap -o --output --target" -- "$cur") )
            return 0
            ;;
        run)
            COMPREPLY=( $(compgen -W "--sourcemap --go-flags" -- "$cur") )
            return 0
            ;;
        test)
            COMPREPLY=( $(compgen -W "-f --filter --failed --go-flags" -- "$cur") )
            return 0
            ;;
        format)
            COMPREPLY=( $(compgen -W "--check" -- "$cur") )
            return 0
            ;;
        add)
            COMPREPLY=( $(compgen -W "--replace --path --script" -- "$cur") )
            return 0
            ;;
        sync)
            COMPREPLY=( $(compgen -W "--script" -- "$cur") )
            return 0
            ;;
        check)
            COMPREPLY=( $(compgen -W "--errors-only --warnings-only --deny --fix --output --target" -- "$cur") )
            return 0
            ;;
        --output)
            COMPREPLY=( $(compgen -W "unix" -- "$cur") )
            return 0
            ;;
        --target)
            COMPREPLY=( $(compgen -W "@TARGETS@" -- "$cur") )
            return 0
            ;;
        --deny)
            COMPREPLY=( $(compgen -W "warnings" -- "$cur") )
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
        'build:Compile a project to a binary'
        'emit:Emit Go code into target/ dir'
        'check:Lint and typecheck a project'
        'format:Format a project'
        'test:Run a project'\''s tests'
        'add:Add a third-party Go dependency'
        'sync:Tidy up project manifest'
        'version:Print compiler version'
        'upgrade:Upgrade compiler toolchain'
        'help:Show help for a command'
        'doc:Browse documentation'
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
                        '--go-flags[Flags passed through to go build]:flags' \
                        {-o,--output}'[Write the binary for a script at this path]:path:_files' \
                        '--target[Compile for another platform]:target:(@TARGETS@)'
                    ;;
                emit)
                    _arguments \
                        '--sourcemap[Include line directives for stack traces]' \
                        {-o,--output}'[Write the Go file for a script at this path]:path:_files' \
                        '--target[Emit for another platform]:target:(@TARGETS@)'
                    ;;
                run)
                    _arguments \
                        '--sourcemap[Include line directives for stack traces]' \
                        '--go-flags[Flags passed through to go build]:flags'
                    ;;
                test)
                    _arguments \
                        {-f,--filter}'[Run only tests whose name contains the pattern]:pattern' \
                        '--failed[Rerun the tests that failed last time]' \
                        '--go-flags[Flags passed through to go test]:flags'
                    ;;
                format)
                    _arguments '--check[Check formatting without modifying]'
                    ;;
                add)
                    _arguments \
                        '--replace[Source the dependency from another module]:module@version' \
                        '--path[Use a local module on disk]:directory:_directories' \
                        '--script[Add the dependency to this script]:file:_files'
                    ;;
                sync)
                    _arguments '--script[Tidy the dependencies of this script]:file:_files'
                    ;;
                check)
                    _arguments \
                        '--errors-only[Show only errors]' \
                        '--warnings-only[Show only warnings]' \
                        '--deny[Fail check if warnings found]:group:(warnings)' \
                        '--fix[Apply lint fixes in place]' \
                        '--output[Machine-readable output]:output:(unix)' \
                        '--target[Check against another platform]:target:(@TARGETS@)'
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
complete -c lis -n __fish_use_subcommand -a build -d 'Compile a project to a binary'
complete -c lis -n __fish_use_subcommand -a emit -d 'Emit Go code into target/ dir'
complete -c lis -n __fish_use_subcommand -a check -d 'Lint and typecheck a project'
complete -c lis -n __fish_use_subcommand -a format -d 'Format a project'
complete -c lis -n __fish_use_subcommand -a test -d 'Run a project\'s tests'
complete -c lis -n __fish_use_subcommand -a add -d 'Add a third-party Go dependency'
complete -c lis -n '__fish_seen_subcommand_from add' -l replace -r -d 'Source the dependency from another module'
complete -c lis -n '__fish_seen_subcommand_from add' -l path -r -d 'Use a local module on disk'
complete -c lis -n '__fish_seen_subcommand_from add' -l script -r -F -d 'Add the dependency to this script'
complete -c lis -n __fish_use_subcommand -a sync -d 'Tidy up project manifest'
complete -c lis -n __fish_use_subcommand -a version -d 'Print compiler version'
complete -c lis -n __fish_use_subcommand -a upgrade -d 'Upgrade compiler toolchain'
complete -c lis -n __fish_use_subcommand -a help -d 'Show help for a command'
complete -c lis -n __fish_use_subcommand -a doc -d 'Browse documentation'
complete -c lis -n __fish_use_subcommand -a learn -d 'Create a new sample project'
complete -c lis -n __fish_use_subcommand -a complete -d 'Shell completion scripts'
complete -c lis -n __fish_use_subcommand -a lsp -d 'Start the language server'

complete -c lis -n '__fish_seen_subcommand_from build' -l sourcemap -d 'Include line directives for stack traces'
complete -c lis -n '__fish_seen_subcommand_from build' -l go-flags -r -d 'Flags passed through to go build'
complete -c lis -n '__fish_seen_subcommand_from build' -s o -l output -r -F -d 'Write the binary for a script at this path'
complete -c lis -n '__fish_seen_subcommand_from build' -l target -r -a '@TARGETS@' -d 'Compile for another platform'
complete -c lis -n '__fish_seen_subcommand_from emit' -l sourcemap -d 'Include line directives for stack traces'
complete -c lis -n '__fish_seen_subcommand_from emit' -s o -l output -r -F -d 'Write the Go file for a script at this path'
complete -c lis -n '__fish_seen_subcommand_from emit' -l target -r -a '@TARGETS@' -d 'Emit for another platform'
complete -c lis -n '__fish_seen_subcommand_from run' -l sourcemap -d 'Include line directives for stack traces'
complete -c lis -n '__fish_seen_subcommand_from run' -l go-flags -r -d 'Flags passed through to go build'
complete -c lis -n '__fish_seen_subcommand_from test' -s f -l filter -r -d 'Run only tests whose name contains the pattern'
complete -c lis -n '__fish_seen_subcommand_from test' -l failed -d 'Rerun the tests that failed last time'
complete -c lis -n '__fish_seen_subcommand_from test' -l go-flags -r -d 'Flags passed through to go test'
complete -c lis -n '__fish_seen_subcommand_from format' -l check -d 'Check formatting without modifying'
complete -c lis -n '__fish_seen_subcommand_from check' -l errors-only -d 'Show only errors'
complete -c lis -n '__fish_seen_subcommand_from check' -l warnings-only -d 'Show only warnings'
complete -c lis -n '__fish_seen_subcommand_from check' -l deny -r -a warnings -d 'Fail check if warnings found'
complete -c lis -n '__fish_seen_subcommand_from check' -l fix -d 'Apply lint fixes in place'
complete -c lis -n '__fish_seen_subcommand_from check' -l output -r -a unix -d 'Machine-readable output'
complete -c lis -n '__fish_seen_subcommand_from check' -l target -r -a '@TARGETS@' -d 'Check against another platform'
complete -c lis -n '__fish_seen_subcommand_from sync' -l script -r -F -d 'Tidy the dependencies of this script'
complete -c lis -n '__fish_seen_subcommand_from doc' -s s -l search -d 'Search across prelude and Go stdlib'
complete -c lis -n '__fish_seen_subcommand_from complete' -a 'bash zsh fish' -d 'Shell type'
complete -c lis -n '__fish_seen_subcommand_from help' -a 'new run build emit check format test add sync version upgrade help doc learn complete lsp' -d 'Command'
"#
}

#[cfg(test)]
mod tests {
    use super::{bash_completions, fish_completions, zsh_completions};

    #[test]
    fn every_shell_completes_upgrade() {
        assert!(bash_completions().contains(" upgrade "));
        assert!(zsh_completions().contains("'upgrade:"));
        assert!(fish_completions().contains("-a upgrade "));
    }
}
