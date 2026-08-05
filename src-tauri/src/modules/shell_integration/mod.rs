//! Shell integration: semantic prompt marking (OSC 133).
//!
//! The shell emits invisible marks around every command — C when one starts, D
//! with its exit status when it ends. That is an exact signal where polling the
//! foreground process group could only guess, and it never mistakes a
//! long-lived TUI (claude, vim, htop) for a command that is still working.
//!
//! Nothing the user owns is modified. zsh is pointed at a Kelyfos-owned ZDOTDIR
//! whose startup files source the user's own first; bash gets an --init-file
//! that does the same. Any other shell simply runs without integration, and the
//! terminal behaves exactly as it did before.

pub mod marks;

use std::path::PathBuf;

use portable_pty::CommandBuilder;

/// Bumped whenever the generated scripts change, so an updated app rewrites
/// them instead of leaving a user on last version's integration.
const SCRIPTS_VERSION: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShellKind {
    Zsh,
    Bash,
    /// fish, nushell, Windows shells: no integration, and no attempt at one.
    Unsupported,
}

pub fn detect(shell_path: &str) -> ShellKind {
    let name = std::path::Path::new(shell_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match name.as_str() {
        "zsh" => ShellKind::Zsh,
        "bash" | "sh" => ShellKind::Bash,
        _ => ShellKind::Unsupported,
    }
}

fn scripts_dir() -> Option<PathBuf> {
    let dir = dirs::data_dir()?.join("kelyfos").join("shell-integration");
    std::fs::create_dir_all(dir.join("zsh")).ok()?;
    Some(dir)
}

/// Write the generated startup files if they are missing or from an older
/// version. Returns the directory holding them.
fn ensure_scripts() -> Option<PathBuf> {
    let dir = scripts_dir()?;
    let stamp = dir.join(".version");
    let current = std::fs::read_to_string(&stamp)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());
    if current == Some(SCRIPTS_VERSION) {
        return Some(dir);
    }

    let zsh = dir.join("zsh");
    std::fs::write(zsh.join(".zshenv"), ZSHENV).ok()?;
    std::fs::write(zsh.join(".zprofile"), ZPROFILE).ok()?;
    std::fs::write(zsh.join(".zshrc"), ZSHRC).ok()?;
    std::fs::write(zsh.join(".zlogin"), ZLOGIN).ok()?;
    std::fs::write(dir.join("bash-init.sh"), BASH_INIT).ok()?;
    std::fs::write(stamp, SCRIPTS_VERSION.to_string()).ok()?;
    Some(dir)
}

/// Point the shell at the generated startup files. Returns whether integration
/// was actually applied — the caller falls back to process polling when not.
///
/// The login-shell flag is this function's business too: zsh keeps `-l` because
/// ZDOTDIR covers every file a login shell reads, while bash cannot combine
/// `-l` with `--init-file`, so its init file sources the login profile itself.
pub fn apply(cmd: &mut CommandBuilder, shell_path: &str, enabled: bool) -> bool {
    if cfg!(windows) || !enabled {
        if !cfg!(windows) {
            cmd.arg("-l");
        }
        return false;
    }

    let kind = detect(shell_path);
    let dir = match (kind, ensure_scripts()) {
        (ShellKind::Unsupported, _) | (_, None) => {
            cmd.arg("-l");
            return false;
        }
        (_, Some(dir)) => dir,
    };

    match kind {
        ShellKind::Zsh => {
            // Remember where the user's own startup files live before taking
            // over ZDOTDIR; the generated ones hand control back afterwards.
            let user_zdotdir = std::env::var("ZDOTDIR")
                .ok()
                .filter(|v| !v.is_empty())
                .or_else(|| dirs::home_dir().map(|h| h.to_string_lossy().into_owned()))
                .unwrap_or_default();
            cmd.env("KELYFOS_USER_ZDOTDIR", user_zdotdir);
            cmd.env("ZDOTDIR", dir.join("zsh"));
            cmd.arg("-l");
            true
        }
        ShellKind::Bash => {
            cmd.arg("--init-file");
            cmd.arg(dir.join("bash-init.sh"));
            true
        }
        ShellKind::Unsupported => unreachable!(),
    }
}

const ZSHENV: &str = r#"# Kelyfos shell integration — generated file, edits are overwritten.
# Loads the user's own .zshenv, then hands ZDOTDIR back to Kelyfos so the rest
# of the startup files (and the prompt marks) still load. The user's .zshenv is
# allowed to set ZDOTDIR itself, which is why it is re-read afterwards.
KELYFOS_ZDOTDIR="$ZDOTDIR"
ZDOTDIR="${KELYFOS_USER_ZDOTDIR:-$HOME}"
[ -f "$ZDOTDIR/.zshenv" ] && . "$ZDOTDIR/.zshenv"
KELYFOS_USER_ZDOTDIR="$ZDOTDIR"
ZDOTDIR="$KELYFOS_ZDOTDIR"
"#;

const ZPROFILE: &str = r#"# Kelyfos shell integration — generated file, edits are overwritten.
[ -f "${KELYFOS_USER_ZDOTDIR:-$HOME}/.zprofile" ] && . "${KELYFOS_USER_ZDOTDIR:-$HOME}/.zprofile"
"#;

const ZLOGIN: &str = r#"# Kelyfos shell integration — generated file, edits are overwritten.
[ -f "${KELYFOS_USER_ZDOTDIR:-$HOME}/.zlogin" ] && . "${KELYFOS_USER_ZDOTDIR:-$HOME}/.zlogin"
"#;

const ZSHRC: &str = r#"# Kelyfos shell integration — generated file, edits are overwritten.

# Hand ZDOTDIR back *before* loading the user's file, not after. Their config
# very likely refers to $ZDOTDIR itself — history file, completion dump, plugin
# paths — and every one of those has to land in their own directory rather than
# in this generated one, or their shell history quietly splits in two. zsh has
# already read everything it needs from here; only .zlogin follows, and it lives
# in their directory too.
ZDOTDIR="${KELYFOS_USER_ZDOTDIR:-$HOME}"

# /etc/zshrc sets HISTFILE from $ZDOTDIR on macOS, and it runs while ZDOTDIR is
# still ours — so defaulting it only when unset would be too late, and the
# user's shell history would accumulate inside this generated folder where they
# will never find it. Redirect it only when it points here; a path the user
# chose themselves is left exactly as it is.
if [ -n "$KELYFOS_ZDOTDIR" ] && [ "$HISTFILE" = "$KELYFOS_ZDOTDIR/.zsh_history" ]; then
  HISTFILE="$ZDOTDIR/.zsh_history"
fi
: ${HISTFILE:="$ZDOTDIR/.zsh_history"}
: ${ZSH_COMPDUMP:="$ZDOTDIR/.zcompdump"}
unset KELYFOS_ZDOTDIR

[ -f "$ZDOTDIR/.zshrc" ] && . "$ZDOTDIR/.zshrc"

# Prompt marks. Every step is guarded: a shell that cannot load add-zsh-hook is
# still a perfectly good shell, and breaking it to light up a status dot would
# be a bad trade.
if [[ -o interactive ]]; then
  if autoload -Uz add-zsh-hook 2>/dev/null; then
    __kelyfos_precmd() {
      local __kelyfos_exit=$?
      # D carries the status of the command that just finished; A marks the
      # start of the next prompt.
      printf '\033]133;D;%s\007\033]133;A\007' "$__kelyfos_exit"
    }
    __kelyfos_preexec() { printf '\033]133;C\007' }
    add-zsh-hook precmd __kelyfos_precmd 2>/dev/null
    add-zsh-hook preexec __kelyfos_preexec 2>/dev/null
  fi
fi
"#;

const BASH_INIT: &str = r#"# Kelyfos shell integration — generated file, edits are overwritten.
# bash refuses to combine --init-file with -l, so the login profile is sourced
# here in the same order bash itself would, keeping PATH and aliases intact.
if [ -f /etc/profile ]; then . /etc/profile; fi
for __kelyfos_f in "$HOME/.bash_profile" "$HOME/.bash_login" "$HOME/.profile"; do
  if [ -f "$__kelyfos_f" ]; then . "$__kelyfos_f"; break; fi
done
unset __kelyfos_f

# Prompt marks. The DEBUG trap fires for the prompt command's own commands too,
# so a flag keeps C to the first command after each prompt.
__kelyfos_at_prompt=1
__kelyfos_prompt() {
  local __kelyfos_exit=$?
  printf '\033]133;D;%s\007\033]133;A\007' "$__kelyfos_exit"
  __kelyfos_at_prompt=1
}
__kelyfos_preexec() {
  [ -n "$COMP_LINE" ] && return
  if [ "$__kelyfos_at_prompt" = 1 ]; then
    __kelyfos_at_prompt=0
    printf '\033]133;C\007'
  fi
}
PROMPT_COMMAND="__kelyfos_prompt${PROMPT_COMMAND:+; $PROMPT_COMMAND}"
trap '__kelyfos_preexec' DEBUG
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_shells_by_basename() {
        assert_eq!(detect("/bin/zsh"), ShellKind::Zsh);
        assert_eq!(detect("/usr/local/bin/bash"), ShellKind::Bash);
        assert_eq!(detect("/opt/homebrew/bin/fish"), ShellKind::Unsupported);
        assert_eq!(detect(""), ShellKind::Unsupported);
    }

    /// The generated zsh files must source the user's own before doing anything
    /// else; a user whose PATH or prompt vanished would rightly uninstall.
    #[test]
    fn zsh_scripts_source_the_users_files() {
        for script in [ZSHENV, ZPROFILE, ZSHRC, ZLOGIN] {
            assert!(script.contains("KELYFOS_USER_ZDOTDIR"));
        }
        assert!(ZSHRC.contains(".zshrc"));
    }

    #[test]
    fn marks_carry_the_exit_status() {
        assert!(ZSHRC.contains("133;D;%s"));
        assert!(BASH_INIT.contains("133;D;%s"));
        assert!(ZSHRC.contains("133;C"));
        assert!(BASH_INIT.contains("133;C"));
    }
}
