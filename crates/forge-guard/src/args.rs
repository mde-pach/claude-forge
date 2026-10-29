//! A command's arguments as options and positionals.
//!
//! An argument starting with `-` (not `-` alone) is an option, up to a `--`, after which all
//! are positionals. An option written `--name=value` is `--name` with that value. An option
//! listed as taking a value (per command, [`value_options`], plus a rule's own `value_flags`)
//! takes the next word as its value. A single-dash word of letters and digits (`-rf`, `-uf`,
//! `-0pi`) is also read as a cluster of one-letter options.

use crate::parse::Word;

#[derive(Debug, Clone, Default)]
pub struct Args<'a> {
    /// Options as written, each with its value (attached with `=`, or the next word).
    pub options: Vec<(&'a str, Option<&'a str>)>,
    pub positionals: Vec<&'a str>,
}

impl<'a> Args<'a> {
    pub fn read(name: &str, words: &'a [Word], extra_value: &[String]) -> Self {
        let table = value_options(name);
        let mut a = Args::default();
        let mut i = 0;
        while i < words.len() {
            let w = words[i].text.as_str();
            if w == "--" {
                a.positionals
                    .extend(words[i + 1..].iter().map(|w| w.text.as_str()));
                break;
            }
            if w.starts_with('-') && w.len() > 1 {
                if let Some((n, v)) = w.split_once('=') {
                    a.options.push((n, Some(v)));
                } else if table.contains(&w) || extra_value.iter().any(|x| x == w) {
                    a.options
                        .push((w, words.get(i + 1).map(|w| w.text.as_str())));
                    i += 1;
                } else {
                    a.options.push((w, None));
                }
            } else {
                a.positionals.push(w);
            }
            i += 1;
        }
        a
    }

    /// Whether option `flag` is given: `--force` (also `--force=x`), `-f` (also inside a
    /// cluster `-uf`, or with a value attached by `.` or `=`: `-i.bak`), `--require-approval=never` (also
    /// as two words).
    pub fn has(&self, flag: &str) -> bool {
        let (name, want) = match flag.split_once('=') {
            Some((n, v)) => (n, Some(v)),
            None => (flag, None),
        };
        let short = name.len() == 2 && name.starts_with('-') && name != "--";
        self.options.iter().any(|&(o, v)| {
            let value_ok = |v: Option<&str>| want.is_none_or(|w| v == Some(w));
            if o == name {
                return value_ok(v);
            }
            if !short || o.starts_with("--") {
                return false;
            }
            let letter = name.as_bytes()[1];
            let body = &o.as_bytes()[1..];
            // a cluster of letters and digits
            if want.is_none() && body.iter().all(|c| c.is_ascii_alphanumeric()) {
                return body.contains(&letter);
            }
            // `-i.bak`: the letter, then a value attached with `.` or `=`
            want.is_none()
                && body.first() == Some(&letter)
                && matches!(body.get(1), Some(b'.' | b'='))
        })
    }
}

/// Options that take the next word as their value, per command name: enough to find a
/// command's subcommand and positionals.
pub fn value_options(name: &str) -> &'static [&'static str] {
    match name {
        "git" => &[
            "-C",
            "-c",
            "--git-dir",
            "--work-tree",
            "--namespace",
            "--exec-path",
            "--config-env",
        ],
        "kubectl" => &[
            "-n",
            "--namespace",
            "--context",
            "--kubeconfig",
            "--cluster",
            "--user",
            "-l",
            "--selector",
            "-o",
            "--output",
            "-f",
            "--filename",
            "-c",
            "--container",
            "-s",
            "--server",
            "--token",
            "--as",
            "--field-selector",
            "-p",
            "--patch",
            "--type",
            "--timeout",
            "--grace-period",
        ],
        "helm" => &[
            "-n",
            "--namespace",
            "--kube-context",
            "--kubeconfig",
            "-f",
            "--values",
            "--set",
            "--version",
            "--timeout",
        ],
        "aws" => &[
            "--profile",
            "--region",
            "--output",
            "--endpoint-url",
            "--query",
            "--color",
            "--ca-bundle",
            "--cli-read-timeout",
            "--cli-connect-timeout",
        ],
        "gcloud" | "az" | "doctl" => &[
            "--project",
            "--zone",
            "--region",
            "--account",
            "--configuration",
            "-o",
            "--output",
            "--subscription",
            "-g",
            "--resource-group",
            "--context",
            "-t",
            "--access-token",
            "--format",
        ],
        "docker" | "docker-compose" | "podman" => &[
            "-H",
            "--host",
            "--context",
            "--config",
            "-l",
            "--log-level",
            "-f",
            "--file",
            "-p",
            "--project-name",
            "--profile",
            "--env-file",
            "--project-directory",
        ],
        "pulumi" => &["-s", "--stack", "-C", "--cwd", "--config-file"],
        "cdk" => &[
            "--profile",
            "-c",
            "--context",
            "-a",
            "--app",
            "-o",
            "--output",
        ],
        "npm" => &[
            "-w",
            "--workspace",
            "--prefix",
            "--registry",
            "--cache",
            "--userconfig",
            "--globalconfig",
            "--loglevel",
            "--omit",
            "--include",
            "--tag",
        ],
        "pnpm" => &["-C", "--dir", "-F", "--filter", "--reporter", "--loglevel"],
        "yarn" => &["--cwd"],
        "bun" => &["--cwd", "-c", "--config", "--env-file", "-r", "--preload"],
        "alembic" => &["-c", "--config", "-n", "--name", "-x"],
        "redis-cli" => &["-n", "-h", "-p", "-a", "-u", "--user", "--pass"],
        "fly" | "flyctl" => &["-a", "--app", "-c", "--config", "-r", "--region"],
        "wrangler" => &["-e", "--env", "-c", "--config"],
        "railway" => &["-s", "--service", "-e", "--environment"],
        "terraform" | "tofu" => &[],
        "prisma" => &["--schema"],
        "drizzle-kit" => &["--config"],
        "mikro-orm"
        | "mikro-orm-esm"
        | "typeorm"
        | "typeorm-ts-node-esm"
        | "typeorm-ts-node-commonjs" => &["-d", "--dataSource", "--config"],
        "psql" => &[
            "-U",
            "-h",
            "-p",
            "-d",
            "-f",
            "--username",
            "--host",
            "--port",
            "--dbname",
        ],
        "mysql" | "mariadb" => &["-u", "-h", "-P", "-D", "--user", "--host", "--port"],
        "chown" | "chmod" => &["--reference"],
        "rm" => &[],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<Word> {
        s.split(' ').map(Word::plain).collect()
    }

    #[test]
    fn options_values_and_clusters() {
        let w = words("-C x push -uf --force-with-lease=main origin -- -n");
        let a = Args::read("git", &w, &[]);
        assert_eq!(a.positionals, ["push", "origin", "-n"]);
        assert!(a.has("-f") && a.has("-u") && a.has("--force-with-lease"));
        assert!(!a.has("--force") && !a.has("-n"));
        let w = words("deploy --require-approval never");
        let a = Args::read("cdk", &w, &["--require-approval".into()]);
        assert!(a.has("--require-approval=never"));
        let w = words("-i.bak -e s/x/y/ uv.lock");
        assert!(Args::read("sed", &w, &[]).has("-i"));
    }
}
