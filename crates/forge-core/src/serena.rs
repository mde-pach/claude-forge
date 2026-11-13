//! Serena, one of Forge's built-in tools (spec §17.8): the code-navigation and symbol-editing
//! MCP server every session of a Forge project gets. This module is what every crate shares
//! about it: the languages and who provides each one's server, the files Forge writes for it,
//! how it is started, and what each of its tools is under Forge's rules.

/// The MCP server's name in `.mcp.json`; its tools are `mcp__serena__<tool>`.
pub const SERVER: &str = "serena";

/// The prefix of Serena's tools as Claude Code names them.
pub const TOOL_PREFIX: &str = "mcp__serena__";

/// The PyPI package and the catalog's tool name.
pub const PACKAGE: &str = "serena-agent";
pub const TOOL: &str = "serena";

/// The language Serena serves under `id`.
pub fn language(id: &str) -> Option<&'static Language> {
    LANGUAGES.iter().find(|l| l.id == id)
}

/// Who provides a language's server. Both phases (Serena running its servers, and later Forge's
/// shared servers) read this one table: giving Forge a server for a language is one entry here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// Forge's hub runs it (`claude-forge lsp <language>`, which Serena starts as the server),
    /// shared by every session of the repository; the hub's command for the language is chosen by
    /// `claude-forge lsp` (the project's own install where it has one), and these pinned tools of
    /// Forge's (catalog names) are installed for it.
    Forge(&'static [&'static str]),
    /// Serena downloads and runs it (`serena download-ls-dependencies`).
    Serena,
}

/// A language Serena serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Language {
    /// Serena's language-server id (`language_servers` in `project.yml`).
    pub id: &'static str,
    /// Active when this pack of Forge's is active.
    pub pack: Option<&'static str>,
    /// Else active when the project has one of these files (its manifest).
    pub files: &'static [&'static str],
    /// The extensions of its source files (the server is checked on one of them).
    pub exts: &'static [&'static str],
    /// For TypeScript: the project's TypeScript major versions it serves.
    pub typescript: TypeScript,
    pub provider: Provider,
}

/// Which TypeScript a TypeScript server serves: up to 6 (`tsserver`, driven by
/// typescript-language-server) or 7 and later (the native compiler's own server, `tsc --lsp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeScript {
    Any,
    UpTo6,
    From7,
}

pub const LANGUAGES: &[Language] = &[
    Language {
        id: "python_ty",
        pack: Some("python"),
        files: &[],
        exts: &["py"],
        typescript: TypeScript::Any,
        provider: Provider::Forge(&["ty"]),
    },
    Language {
        id: "typescript",
        pack: Some("typescript"),
        files: &[],
        exts: &["ts", "tsx", "js", "jsx", "mts", "cts"],
        typescript: TypeScript::UpTo6,
        provider: Provider::Forge(&["typescript-language-server", "typescript"]),
    },
    Language {
        id: "typescript_tsgo",
        pack: Some("typescript"),
        files: &[],
        exts: &["ts", "tsx", "js", "jsx", "mts", "cts"],
        typescript: TypeScript::From7,
        provider: Provider::Forge(&["typescript-native"]),
    },
    Language {
        id: "terraform",
        pack: Some("terraform"),
        files: &[],
        exts: &["tf"],
        typescript: TypeScript::Any,
        provider: Provider::Serena,
    },
    Language {
        id: "rust",
        pack: None,
        files: &["Cargo.toml"],
        exts: &["rs"],
        typescript: TypeScript::Any,
        provider: Provider::Serena,
    },
    Language {
        id: "go",
        pack: None,
        files: &["go.mod"],
        exts: &["go"],
        typescript: TypeScript::Any,
        provider: Provider::Serena,
    },
    Language {
        id: "java",
        pack: None,
        files: &["pom.xml", "build.gradle", "build.gradle.kts"],
        exts: &["java"],
        typescript: TypeScript::Any,
        provider: Provider::Serena,
    },
    Language {
        id: "ruby",
        pack: None,
        files: &["Gemfile"],
        exts: &["rb"],
        typescript: TypeScript::Any,
        provider: Provider::Serena,
    },
    Language {
        id: "php",
        pack: None,
        files: &["composer.json"],
        exts: &["php"],
        typescript: TypeScript::Any,
        provider: Provider::Serena,
    },
    Language {
        id: "csharp",
        pack: None,
        files: &["*.csproj", "*.sln"],
        exts: &["cs"],
        typescript: TypeScript::Any,
        provider: Provider::Serena,
    },
    Language {
        id: "swift",
        pack: None,
        files: &["Package.swift"],
        exts: &["swift"],
        typescript: TypeScript::Any,
        provider: Provider::Serena,
    },
];

/// The project's languages: those whose pack is active or whose manifest the project has
/// (`has_file` takes a glob of the repository root's files).
pub fn languages(
    pack_active: impl Fn(&str) -> bool,
    has_file: impl Fn(&str) -> bool,
    typescript_major: Option<u32>,
) -> Vec<Language> {
    LANGUAGES
        .iter()
        .filter(|l| l.pack.is_some_and(&pack_active) || l.files.iter().any(|f| has_file(f)))
        .filter(|l| match l.typescript {
            TypeScript::Any => true,
            TypeScript::UpTo6 => typescript_major.is_none_or(|m| m <= 6),
            TypeScript::From7 => typescript_major.is_some_and(|m| m >= 7),
        })
        .copied()
        .collect()
}

/// The major version of the project's own TypeScript at `root`: the installed one
/// (`node_modules/typescript/package.json`), else the one `package.json` asks for (its first
/// number); None when it has none.
pub fn typescript_major(root: &std::path::Path) -> Option<u32> {
    let read = |p: std::path::PathBuf| -> Option<serde_json::Value> {
        serde_json::from_slice(&std::fs::read(p).ok()?).ok()
    };
    let major = |v: &str| -> Option<u32> {
        let digits: String = v
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse().ok()
    };
    if let Some(v) = read(root.join("node_modules/typescript/package.json")) {
        if let Some(m) = v["version"].as_str().and_then(major) {
            return Some(m);
        }
    }
    let pkg = read(root.join("package.json"))?;
    ["devDependencies", "dependencies", "peerDependencies"]
        .iter()
        .find_map(|k| pkg[k]["typescript"].as_str().and_then(major))
}

/// Serena's project folder in the repository.
pub const DIR: &str = ".serena";
pub const PROJECT_FILE: &str = ".serena/project.yml";
pub const GITIGNORE_FILE: &str = ".serena/.gitignore";

/// Serena's own `.gitignore` for its folder: its symbol cache and local overrides.
pub const GITIGNORE: &str = "/cache\n/project.local.yml\n";

/// `.serena/project.yml`: every key Serena 2.1 reads, so it never re-saves the file.
pub fn project_yml(name: &str, languages: &[Language]) -> String {
    let mut s = String::from(
        "# Written by the project's tooling; Serena's project configuration (see .mcp.json).\n",
    );
    s.push_str(&format!("project_name: {}\n", yaml_str(name)));
    s.push_str("ignore_all_files_in_gitignore: true\nread_only: false\nlanguage_servers:\n");
    for l in languages {
        s.push_str(&format!("- {}\n", l.id));
    }
    s.push_str(concat!(
        "default_modes:\n",
        "added_modes:\n",
        "included_apis: []\n",
        "excluded_apis: []\n",
        "excluded_tools: []\n",
        "included_optional_tools: []\n",
        "fixed_tools: []\n",
        "symbol_info_budget:\n",
        "language_backend:\n",
        "agent_interface:\n",
        "line_ending:\n",
        "read_only_memory_patterns: []\n",
        "ignored_memory_patterns: []\n",
        "ls_specific_settings: {}\n",
        "ignored_paths: []\n",
        "ls_workspace_folders:\n",
        "- .\n",
        "ls_additional_workspace_folders: []\n",
        "initial_prompt: ''\n",
        "encoding: utf-8\n",
        "activation_command:\n",
        "activation_command_timeout: 180.0\n",
    ));
    s
}

/// Serena's machine configuration (`<SERENA_HOME>/serena_config.yml`, in Forge's cache) with
/// `ls_specific_settings` giving each server Forge provides its exact command
/// (`(language id, command)`), the rest of the file as it is. Serena reads a server's command only
/// from here: a project it does not trust (Forge trusts none) cannot choose what Serena runs.
pub fn global_config(existing: &str, servers: &[(&str, Vec<String>)]) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in existing.lines() {
        if line.starts_with("ls_specific_settings:") {
            skipping = true;
            continue;
        }
        if skipping && (line.starts_with(' ') || line.starts_with('\t') || line.is_empty()) {
            continue;
        }
        skipping = false;
        out.push_str(line);
        out.push('\n');
    }
    // a file Serena did not write yet: the one key it requires (it fills in the rest)
    if !out.lines().any(|l| l.starts_with("projects:")) {
        out.insert_str(0, "projects: []\n");
    }
    if servers.is_empty() {
        out.push_str("ls_specific_settings: {}\n");
        return out;
    }
    out.push_str("ls_specific_settings:\n");
    for (id, cmd) in servers {
        let list = serde_json::to_string(cmd).unwrap_or_else(|_| "[]".into());
        out.push_str(&format!(
            "  {id}:\n    ls_base_cmd: {list}\n    ls_args: []\n"
        ));
    }
    out
}

/// A YAML double-quoted scalar.
fn yaml_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// The arguments after `serena`: the MCP server on the project, one tool per operation, Serena's
/// context for Claude Code, and no dashboard or window.
pub fn server_args(project: &str) -> Vec<String> {
    [
        "start-mcp-server",
        "--project",
        project,
        "--context",
        "claude-code",
        "--agent-interface",
        "tools",
        "--enable-web-dashboard",
        "false",
        "--open-web-dashboard",
        "false",
        "--enable-gui-log-window",
        "false",
        "--log-level",
        "WARNING",
    ]
    .map(String::from)
    .to_vec()
}

/// The `.mcp.json` entry's shell command: Forge's launcher where Forge is installed, else the
/// same pinned Serena run by uv, with the same arguments on the project around the directory
/// Claude Code started in (`version`: the pinned one).
pub fn mcp_command(version: &str) -> String {
    let args = server_args(".")
        .join(" ")
        .replace("--project .", "--project-from-cwd");
    format!(
        "command -v claude-forge >/dev/null 2>&1 && exec claude-forge mcp serena; exec uvx --python 3.12 --from {PACKAGE}=={version} serena {args}"
    )
}

/// What a Serena tool is under Forge's rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Reads or navigates: the internals refusal on the files it names (as Read, Grep, Glob).
    Read,
    /// Edits the one file it names (as Edit and Write): the test-edit ask before, the checks after.
    Edit,
    /// May edit any number of files (as a Bash command that edits): snapshot before, checks after.
    MultiEdit,
    /// Serena's notes in `.serena/memories/`, and its own instructions: nothing more.
    Memory,
    /// Refused: Forge fixes the project and the configuration.
    Config,
    /// Refused: commands run with Bash, under the guard, the sandbox and the permission rules.
    Shell,
}

/// The kind of a Serena tool (its name without [`TOOL_PREFIX`]). A tool this table does not know
/// (a newer Serena) is taken as one that may edit any file.
pub fn kind(tool: &str) -> Kind {
    match tool {
        "find_symbol"
        | "find_referencing_symbols"
        | "get_symbols_overview"
        | "find_declaration"
        | "find_implementations"
        | "get_diagnostics_for_file"
        | "read_file"
        | "list_dir"
        | "find_file"
        | "search_for_pattern" => Kind::Read,
        "replace_content"
        | "replace_symbol_body"
        | "insert_before_symbol"
        | "insert_after_symbol"
        | "create_text_file" => Kind::Edit,
        "write_memory"
        | "read_memory"
        | "list_memories"
        | "delete_memory"
        | "rename_memory"
        | "edit_memory"
        | "onboarding"
        | "initial_instructions"
        | "check_onboarding_performed" => Kind::Memory,
        "activate_project" | "remove_project" | "get_current_config" | "open_dashboard"
        | "switch_modes" => Kind::Config,
        "execute_shell_command" => Kind::Shell,
        _ => Kind::MultiEdit,
    }
}

/// The Serena tool a Claude Code tool name is, if any (`mcp__serena__find_symbol` → `find_symbol`).
pub fn tool_of(name: &str) -> Option<&str> {
    name.strip_prefix(TOOL_PREFIX)
}

/// The navigation tools Forge's own agents get (read-only), as Claude Code names them.
pub fn read_tools() -> Vec<String> {
    [
        "find_symbol",
        "find_referencing_symbols",
        "get_symbols_overview",
        "find_declaration",
        "find_implementations",
    ]
    .iter()
    .map(|t| format!("{TOOL_PREFIX}{t}"))
    .collect()
}

pub const SHELL_REASON: &str = "Run commands with Bash in this project.";
pub const CONFIG_REASON: &str =
    "Serena's project and configuration are fixed in this project; use its code tools.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_from_packs_and_manifests() {
        let l = languages(|p| p == "python", |f| f == "go.mod", None);
        let ids: Vec<&str> = l.iter().map(|l| l.id).collect();
        assert_eq!(ids, ["python_ty", "go"]);
        assert_eq!(l[0].provider, Provider::Forge(&["ty"]));
        // TypeScript: one server by the project's TypeScript (none installed: up to 6)
        let ts = |m| -> Vec<&str> {
            languages(|p| p == "typescript", |_| false, m)
                .iter()
                .map(|l| l.id)
                .collect()
        };
        assert_eq!(ts(None), ["typescript"]);
        assert_eq!(ts(Some(5)), ["typescript"]);
        assert_eq!(ts(Some(7)), ["typescript_tsgo"]);
    }

    #[test]
    fn project_yml_lists_every_key() {
        let y = project_yml("app", &languages(|p| p == "python", |_| false, None));
        assert!(y.contains("project_name: \"app\"\n"));
        assert!(y.contains("language_servers:\n- python_ty\n"));
        assert!(y.contains("activation_command_timeout: 180.0\n"));
    }

    #[test]
    fn global_config_gives_forge_s_commands_and_keeps_the_rest() {
        let before = "projects: []\nweb_dashboard: false\nls_specific_settings:\n  python_ty:\n    ls_path: /old\n\nlog_level: 30\n";
        let cmd = vec!["/c/forge".to_string(), "lsp".into(), "python_ty".into()];
        let after = global_config(before, &[("python_ty", cmd.clone())]);
        assert_eq!(
            after,
            "projects: []\nweb_dashboard: false\nlog_level: 30\nls_specific_settings:\n  python_ty:\n    ls_base_cmd: [\"/c/forge\",\"lsp\",\"python_ty\"]\n    ls_args: []\n"
        );
        assert_eq!(global_config(&after, &[("python_ty", cmd)]), after);
        // a file Serena has not written yet gets the key it requires
        assert!(global_config("", &[]).starts_with("projects: []\n"));
    }

    #[test]
    fn the_project_s_typescript_major() {
        let d = std::env::temp_dir().join(format!("forge-ts-major-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("node_modules/typescript")).unwrap();
        assert_eq!(typescript_major(&d), None);
        std::fs::write(
            d.join("package.json"),
            r#"{"devDependencies":{"typescript":"^7.0.2"}}"#,
        )
        .unwrap();
        assert_eq!(typescript_major(&d), Some(7));
        // the installed one wins over what package.json asks for
        std::fs::write(
            d.join("node_modules/typescript/package.json"),
            r#"{"version":"5.9.3"}"#,
        )
        .unwrap();
        assert_eq!(typescript_major(&d), Some(5));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn kinds() {
        assert_eq!(kind("find_symbol"), Kind::Read);
        assert_eq!(kind("replace_symbol_body"), Kind::Edit);
        assert_eq!(kind("rename_symbol"), Kind::MultiEdit);
        assert_eq!(kind("something_new"), Kind::MultiEdit);
        assert_eq!(kind("execute_shell_command"), Kind::Shell);
        assert_eq!(tool_of("mcp__serena__find_symbol"), Some("find_symbol"));
        assert!(
            mcp_command("2.1.0")
                .contains("serena-agent==2.1.0 serena start-mcp-server --project-from-cwd")
        );
    }
}
