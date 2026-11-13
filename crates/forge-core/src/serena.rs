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

/// Who provides a language's server. Both phases (Serena running its servers, and later Forge's
/// shared servers) read this one table: giving Forge a server for a language is one entry here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// One of Forge's pinned tools (its catalog name), passed to Serena as the server's path.
    Forge(&'static str),
    /// The project's own installation (its `node_modules/typescript`), which Serena loads.
    Project,
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
    pub provider: Provider,
}

pub const LANGUAGES: &[Language] = &[
    Language {
        id: "python_ty",
        pack: Some("python"),
        files: &[],
        exts: &["py"],
        provider: Provider::Forge("ty"),
    },
    Language {
        id: "typescript",
        pack: Some("typescript"),
        files: &[],
        exts: &["ts", "tsx", "js", "jsx", "mts", "cts"],
        provider: Provider::Project,
    },
    Language {
        id: "terraform",
        pack: Some("terraform"),
        files: &[],
        exts: &["tf"],
        provider: Provider::Serena,
    },
    Language {
        id: "rust",
        pack: None,
        files: &["Cargo.toml"],
        exts: &["rs"],
        provider: Provider::Serena,
    },
    Language {
        id: "go",
        pack: None,
        files: &["go.mod"],
        exts: &["go"],
        provider: Provider::Serena,
    },
    Language {
        id: "java",
        pack: None,
        files: &["pom.xml", "build.gradle", "build.gradle.kts"],
        exts: &["java"],
        provider: Provider::Serena,
    },
    Language {
        id: "ruby",
        pack: None,
        files: &["Gemfile"],
        exts: &["rb"],
        provider: Provider::Serena,
    },
    Language {
        id: "php",
        pack: None,
        files: &["composer.json"],
        exts: &["php"],
        provider: Provider::Serena,
    },
    Language {
        id: "csharp",
        pack: None,
        files: &["*.csproj", "*.sln"],
        exts: &["cs"],
        provider: Provider::Serena,
    },
    Language {
        id: "swift",
        pack: None,
        files: &["Package.swift"],
        exts: &["swift"],
        provider: Provider::Serena,
    },
];

/// The project's languages: those whose pack is active or whose manifest the project has
/// (`has_file` takes a glob of the repository root's files).
pub fn languages(
    pack_active: impl Fn(&str) -> bool,
    has_file: impl Fn(&str) -> bool,
) -> Vec<Language> {
    LANGUAGES
        .iter()
        .filter(|l| l.pack.is_some_and(&pack_active) || l.files.iter().any(|f| has_file(f)))
        .copied()
        .collect()
}

/// Serena's project folder in the repository.
pub const DIR: &str = ".serena";
pub const PROJECT_FILE: &str = ".serena/project.yml";
pub const GITIGNORE_FILE: &str = ".serena/.gitignore";
/// Machine-dependent settings, written at each launch and never committed.
pub const LOCAL_FILE: &str = ".serena/project.local.yml";

/// Serena's own `.gitignore` for its folder: its symbol cache and the local settings.
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

/// `.serena/project.local.yml`: the paths of the servers Forge provides (`(language id, path)`).
pub fn local_yml(servers: &[(&str, String)]) -> String {
    let mut s =
        String::from("# Written at each start of Serena; machine-dependent, not committed.\n");
    if servers.is_empty() {
        s.push_str("ls_specific_settings: {}\n");
        return s;
    }
    s.push_str("ls_specific_settings:\n");
    for (id, path) in servers {
        s.push_str(&format!("  {id}:\n    ls_path: {}\n", yaml_str(path)));
    }
    s
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
        let l = languages(|p| p == "python", |f| f == "go.mod");
        let ids: Vec<&str> = l.iter().map(|l| l.id).collect();
        assert_eq!(ids, ["python_ty", "go"]);
        assert_eq!(l[0].provider, Provider::Forge("ty"));
    }

    #[test]
    fn project_yml_lists_every_key() {
        let y = project_yml("app", &languages(|p| p == "python", |_| false));
        assert!(y.contains("project_name: \"app\"\n"));
        assert!(y.contains("language_servers:\n- python_ty\n"));
        assert!(y.contains("activation_command_timeout: 180.0\n"));
    }

    #[test]
    fn local_yml_points_at_forge_s_servers() {
        let y = local_yml(&[("python_ty", "/c/tools/ty/0.0.86/bin/ty".into())]);
        assert!(y.contains("  python_ty:\n    ls_path: \"/c/tools/ty/0.0.86/bin/ty\"\n"));
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
