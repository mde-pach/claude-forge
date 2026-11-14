//! The secret files a Docker build context leaves out, as `forge apply` offers them (§3.5) where
//! the docker pack's `docker/dockerignore` is active: the dotenv files (templates kept), and the
//! keystores, private keys and credential files `secrets/no-credential-files` keeps out of git.
//! A build that copies the context (`COPY . .`) then never puts them in an image layer.

/// The dotenv lines: every `.env` and `.env.*` at any depth, the templates (`@env-template`) kept.
pub const ENV_LINES: &[&str] = &[
    "**/.env",
    "**/.env.*",
    "!**/.env.example",
    "!**/.env.sample",
    "!**/.env.template",
    "!**/.env.dist",
];

/// The credential files (`secrets/no-credential-files`'s list, but `.npmrc`: a build that installs
/// from a private registry copies it on purpose).
pub const SECRET_LINES: &[&str] = &[
    "**/*.p12",
    "**/*.pfx",
    "**/*.jks",
    "**/*.keystore",
    "**/*.key",
    "**/id_rsa",
    "**/id_ecdsa",
    "**/id_ed25519",
    "**/.pypirc",
    "**/.netrc",
    "**/.git-credentials",
];

const HEADER: &str = "# Secrets stay out of the build context";

/// A pattern as Docker reads it: no surrounding blanks, no leading `/`.
fn norm(l: &str) -> &str {
    let t = l.trim();
    match t.strip_prefix('!') {
        Some(_) => t,
        None => t.trim_start_matches('/'),
    }
}

/// A `.dockerignore` (`None`: there is none yet) with the lines it lacks, and the lines added.
/// The dotenv lines come only where the file names no `.env` pattern yet: one it has is the
/// project's choice (which templates the build copies), and `docker/dockerignore` checks it.
/// `None`: nothing to add, or a file that excludes everything (`*`, `**`) and lists what goes in.
pub fn with_secret_excludes(text: Option<&str>) -> Option<(String, Vec<&'static str>)> {
    let t = text.unwrap_or("");
    let lines: Vec<&str> = t.lines().map(norm).collect();
    if lines.iter().any(|l| *l == "*" || *l == "**") {
        return None;
    }
    let mut add: Vec<&'static str> = Vec::new();
    let mentions_env = lines
        .iter()
        .any(|l| !l.starts_with('#') && l.contains(".env"));
    if !mentions_env {
        add.extend(ENV_LINES);
    }
    for s in SECRET_LINES {
        let bare = s.trim_start_matches("**/");
        if !lines.iter().any(|l| *l == *s || *l == bare) {
            add.push(s);
        }
    }
    if add.is_empty() {
        return None;
    }
    let mut out = t.to_string();
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(HEADER);
    out.push('\n');
    for l in &add {
        out.push_str(l);
        out.push('\n');
    }
    Some((out, add))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_the_file() {
        let (text, added) = with_secret_excludes(None).unwrap();
        assert!(text.starts_with(
            "# Secrets stay out of the build context\n**/.env\n**/.env.*\n!**/.env.example\n"
        ));
        assert!(text.ends_with("**/.git-credentials\n"));
        assert_eq!(added.len(), ENV_LINES.len() + SECRET_LINES.len());
    }

    #[test]
    fn adds_only_what_is_missing() {
        let have = ".git\nnode_modules\n.env*\n*.key\n/id_rsa";
        let (text, added) = with_secret_excludes(Some(have)).unwrap();
        // the project's .env* is its choice: no dotenv line, no template re-included
        assert!(!added.iter().any(|l| l.contains(".env")), "{added:?}");
        assert!(!added.contains(&"**/*.key") && !added.contains(&"**/id_rsa"));
        assert!(text.starts_with(".git\nnode_modules\n.env*\n*.key\n/id_rsa\n\n# Secrets"));
        let all = format!("{}\n{}\n", ENV_LINES.join("\n"), SECRET_LINES.join("\n"));
        assert!(with_secret_excludes(Some(&all)).is_none());
        // an allow-list: nothing goes in that it does not name
        assert!(with_secret_excludes(Some("*\n!src\n!package.json\n")).is_none());
    }
}
