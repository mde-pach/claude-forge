//! Dockerfiles (`templates/engine/docker.sh`): hadolint and Forge's own Dockerfile checks.

use std::sync::LazyLock;

use forge_core::{NoteKind, Report};
use regex::Regex;

use crate::cx::{self, Cx, Scratch, dir_of, join, path_arg};

macro_rules! re {
    ($name:ident, $src:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($src).expect("regex"));
    };
}

/// `forge_dockerfiles`: the in-scope files in file role @dockerfile.
fn dockerfiles(cx: &Cx<'_>) -> Vec<String> {
    cx.scope_files()
        .into_iter()
        .filter(|f| cx.role("dockerfile", f))
        .collect()
}

/// One instruction: its first line and its text (continuation lines joined, comments dropped).
struct Record {
    line: u32,
    text: String,
}

impl Record {
    /// The instruction, upper-cased ("" when the line starts with whitespace, as the engine read it).
    fn instr(&self) -> String {
        self.text
            .split(|c: char| c.is_whitespace())
            .next()
            .unwrap_or("")
            .to_uppercase()
    }

    /// The arguments: the text after the instruction word.
    fn args(&self) -> String {
        re!(HEAD, r"^\s*[A-Za-z]+\s*");
        HEAD.replace(&self.text, "").into_owned()
    }
}

/// `forge_dockerfile_records`.
fn records(text: &str) -> Vec<Record> {
    re!(BLANK, r"^\s*(#|$)");
    re!(COMMENT, r"^\s*#");
    re!(CONT, r"\\\s*$");
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    let mut start = 0;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        match &cur {
            None => {
                if BLANK.is_match(line) {
                    continue;
                }
                start = i as u32 + 1;
            }
            Some(_) if COMMENT.is_match(line) => continue,
            Some(_) => {}
        }
        let mut c = cur.take().unwrap_or_default();
        if let Some(m) = CONT.find(line) {
            c.push_str(&line[..m.start()]);
            c.push(' ');
            cur = Some(c);
            continue;
        }
        c.push_str(line);
        if !c.is_empty() {
            out.push(Record {
                line: start,
                text: c,
            });
        }
    }
    if let Some(c) = cur.filter(|c| !c.is_empty()) {
        out.push(Record {
            line: start,
            text: c,
        });
    }
    out
}

fn words(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}

/// A RUN's arguments without its `--mount=…` style flags.
fn strip_flags(args: &str) -> String {
    re!(FLAG, r"^--\S+\s+");
    let mut a = args.to_string();
    while FLAG.is_match(&a) {
        a = FLAG.replace(&a, "").into_owned();
    }
    a
}

/// The commands of a RUN (split on `&&`, `||`, `;`), whitespace normalised, leading variable
/// assignments and `sudo` removed.
fn commands(args: &str) -> Vec<String> {
    re!(SPLIT, r"&&|\|\||;");
    re!(LEAD, r"^([A-Za-z_][A-Za-z0-9_]*=\S* |sudo )");
    SPLIT
        .split(&strip_flags(args))
        .map(|c| {
            let mut c = words(c).join(" ");
            while LEAD.is_match(&c) {
                c = c
                    .split_once(' ')
                    .map(|x| x.1.to_string())
                    .unwrap_or_default();
            }
            c
        })
        .collect()
}

fn each_dockerfile(cx: &Cx<'_>, mut f: impl FnMut(&str, &str)) {
    for df in dockerfiles(cx) {
        if let Some(text) = cx.now(&df) {
            f(&df, &text);
        }
    }
}

// ---------------------------------------------------------------- hadolint

/// `forge_hadolint <code>...`: "path:line CODE: message" for the named hadolint codes. hadolint
/// runs once per run for the same Dockerfiles (its output kept in the run's scratch directory);
/// a Dockerfile it cannot parse is reported once per run, at its line.
pub fn hadolint(cx: &Cx<'_>, codes: &[String]) -> Report {
    let mut r = Report::default();
    let files = dockerfiles(cx);
    if files.is_empty() {
        return r;
    }
    let scratch = cx.scratch();
    let out = match hadolint_output(cx, &files, &scratch) {
        Ok(o) => o,
        Err(note) => {
            r.notes.push(cx.note(note.1, note.0));
            return r;
        }
    };
    re!(PARSE, r"^(\S+):([0-9]+):([0-9]+) (.*)$");
    re!(FOUND, r"^(\S+):([0-9]+) ([A-Z]+[0-9]+) [a-z]+: (.*)$");
    let shared = matches!(scratch, Scratch::Run(_));
    let mark = scratch.path().join("infra-hadolint.parse-reported");
    for l in out.lines() {
        if let Some(c) = PARSE.captures(l) {
            if shared && mark.exists() {
                continue;
            }
            r.findings.push(cx.at(
                &c[1],
                c[2].parse().unwrap_or(0),
                format!(
                    "this Dockerfile does not parse (column {}: {}); fix its syntax",
                    &c[3],
                    c[4].trim()
                ),
            ));
            continue;
        }
        if let Some(c) = FOUND.captures(l)
            && codes.iter().any(|x| x == &c[3])
        {
            r.findings.push(cx.at(
                &c[1],
                c[2].parse().unwrap_or(0),
                format!("{}: {}", &c[3], &c[4]),
            ));
        }
    }
    if shared
        && r.findings
            .iter()
            .any(|f| f.message.contains("does not parse"))
    {
        let _ = std::fs::write(&mark, b"");
    }
    r
}

/// hadolint's output for `files`, from the run's cache when it already ran on them.
fn hadolint_output(
    cx: &Cx<'_>,
    files: &[String],
    scratch: &Scratch,
) -> Result<String, (String, NoteKind)> {
    let cache = scratch.path().join("infra-hadolint.json");
    let key = files.join("\n");
    if let Ok(t) = std::fs::read_to_string(&cache)
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&t)
        && v["files"] == key
    {
        if let Some(o) = v["out"].as_str() {
            return Ok(o.to_string());
        }
        let kind = if v["missing"].as_bool() == Some(true) {
            NoteKind::MissingTool
        } else {
            NoteKind::ForgeFailure
        };
        return Err((v["why"].as_str().unwrap_or("").to_string(), kind));
    }
    let res = run_hadolint(cx, files, scratch);
    let v = match &res {
        Ok(o) => serde_json::json!({"files": key, "out": o}),
        Err((why, k)) => {
            serde_json::json!({"files": key, "why": why, "missing": *k == NoteKind::MissingTool})
        }
    };
    let _ = std::fs::write(&cache, v.to_string());
    res
}

fn run_hadolint(
    cx: &Cx<'_>,
    files: &[String],
    scratch: &Scratch,
) -> Result<String, (String, NoteKind)> {
    let bin = cx.pinned("hadolint").map_err(|n| (n.text, n.kind))?;
    let cfg = scratch.path().join("infra-hadolint.yaml");
    std::fs::write(&cfg, "ignored: []\n").map_err(|e| {
        (
            format!("hadolint could not run (its config: {e})"),
            NoteKind::ForgeFailure,
        )
    })?;
    let mut argv = cx::with(
        &bin,
        vec![
            "--disable-ignore-pragma".into(),
            "--no-fail".into(),
            "--no-color".into(),
            "-f".into(),
            "tty".into(),
            "-c".into(),
            path_arg(&cfg),
        ],
    );
    argv.extend(files.iter().cloned());
    let o = cx::run(&argv, cx.root(), &[]);
    // --no-fail: 0 whatever it finds; 1 it could not run (an unreadable file)
    if o.code != Some(0) {
        let why = o.error.clone().unwrap_or_else(|| {
            o.both()
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .to_string()
        });
        return Err((
            format!("hadolint could not run ({})", why.trim()),
            NoteKind::ForgeFailure,
        ));
    }
    Ok(o.both())
}

// ---------------------------------------------------------------- Forge's own checks

/// `forge_docker_nonroot`: the final stage of each Dockerfile runs as a non-root USER, set in
/// that stage or inherited from the named stage it builds on; images that run non-root by
/// default (cgr.dev/, dhi.io/, *nonroot*, *unprivileged*, *rootless*) are exempt.
pub fn nonroot(cx: &Cx<'_>, _args: &[String]) -> Report {
    re!(EXEMPT_REG, r"^(cgr\.dev|dhi\.io)/");
    re!(EXEMPT_NAME, r"nonroot|unprivileged|rootless");
    re!(ROOT, r"^(root|0)(:.*)?$");
    let mut r = Report::default();
    each_dockerfile(cx, |df, text| {
        // per stage: image, name, user
        let mut stages: Vec<(String, String, String)> = Vec::new();
        for rec in records(text) {
            let args = rec.args();
            let w = words(&args);
            match rec.instr().as_str() {
                "FROM" => {
                    let i = w
                        .iter()
                        .position(|x| !x.starts_with("--"))
                        .unwrap_or(w.len());
                    let img = w.get(i).copied().unwrap_or("").to_string();
                    let name = if w.get(i + 1).map(|x| x.to_lowercase()) == Some("as".into()) {
                        w.get(i + 2).map(|x| x.to_lowercase()).unwrap_or_default()
                    } else {
                        String::new()
                    };
                    stages.push((img, name, String::new()));
                }
                "USER" => {
                    if let Some(s) = stages.last_mut() {
                        s.2 = w.first().copied().unwrap_or("").to_string();
                    }
                }
                _ => {}
            }
        }
        let Some(last) = stages.len().checked_sub(1) else {
            return;
        };
        let (mut s, mut u, mut b) = (last, stages[last].2.clone(), stages[last].0.clone());
        for _ in 0..50 {
            if !u.is_empty() {
                break;
            }
            let Some(k) = (0..s)
                .rev()
                .find(|&j| !stages[j].1.is_empty() && stages[j].1 == b.to_lowercase())
            else {
                break;
            };
            s = k;
            u = stages[k].2.clone();
            b = stages[k].0.clone();
        }
        if u.is_empty() {
            if EXEMPT_REG.is_match(&b) || EXEMPT_NAME.is_match(&b.to_lowercase()) {
                return;
            }
            r.findings.push(cx.at(
                df,
                0,
                "the final stage has no USER (it runs as root); add a non-root USER",
            ));
        } else if ROOT.is_match(&u) {
            r.findings.push(cx.at(
                df,
                0,
                format!("the final stage runs as USER {u}; use a non-root user"),
            ));
        }
    });
    r
}

/// `forge_docker_secrets`: no ARG or ENV named like a secret; *_FILE names are fine.
pub fn secrets(cx: &Cx<'_>, _args: &[String]) -> Report {
    re!(
        SECRET,
        r"(PASSWORD|SECRET|TOKEN|API_?KEY|PRIVATE_KEY|CREDENTIALS)"
    );
    let mut r = Report::default();
    each_dockerfile(cx, |df, text| {
        for rec in records(text) {
            let instr = rec.instr();
            if instr != "ARG" && instr != "ENV" {
                continue;
            }
            let args = rec.args();
            let w = words(&args);
            for (i, word) in w.iter().enumerate() {
                let name = word.split('=').next().unwrap_or("");
                // ENV key value: the key only
                if instr == "ENV" && i > 0 && !w[0].contains('=') {
                    break;
                }
                let up = name.to_uppercase();
                if SECRET.is_match(&up) && !up.ends_with("_FILE") {
                    r.findings.push(cx.at(
                        df,
                        rec.line,
                        format!(
                            "{instr} {name} puts a secret in the image or its history; use RUN --mount=type=secret,id=..."
                        ),
                    ));
                }
                if instr == "ARG" {
                    break;
                }
            }
        }
    });
    r
}

/// `forge_docker_locked`: RUN installs from the lock file and never adds packages.
pub fn locked(cx: &Cx<'_>, _args: &[String]) -> Report {
    re!(NPM, r"^npm (install|i|add)( |$)");
    re!(BUN_I, r"^bun (install|i)( |$)");
    re!(BUN_ADD, r"^bun add( |$)");
    re!(UV_SYNC, r"^uv sync( |$)");
    re!(UV_ADD, r"^uv add( |$)");
    re!(PNPM, r"^pnpm (install|i)( |$)");
    re!(YARN, r"^yarn( install)?( -[^ ]*)*$");
    fn why(c: &str) -> &'static str {
        let t = format!(" {c} ");
        let has = |w: &[&str]| w.iter().any(|x| t.contains(&format!(" {x} ")));
        if NPM.is_match(c) && !has(&["-g", "--global"]) {
            "use npm ci"
        } else if BUN_I.is_match(c) && !has(&["--frozen-lockfile"]) {
            "add --frozen-lockfile"
        } else if BUN_ADD.is_match(c) {
            "add dependencies to package.json and bun.lock, not in the image"
        } else if UV_SYNC.is_match(c) && !has(&["--locked", "--frozen"]) {
            "add --locked (or --frozen)"
        } else if UV_ADD.is_match(c) {
            "add dependencies to pyproject.toml and uv.lock, not in the image"
        } else if PNPM.is_match(c) && !has(&["--frozen-lockfile"]) {
            "add --frozen-lockfile"
        } else if YARN.is_match(c) && !has(&["--immutable", "--frozen-lockfile"]) {
            "add --immutable (Yarn 2+) or --frozen-lockfile"
        } else {
            ""
        }
    }
    let mut r = Report::default();
    each_dockerfile(cx, |df, text| {
        for rec in records(text) {
            if rec.instr() != "RUN" {
                continue;
            }
            for c in commands(&rec.args()) {
                let w = why(&c);
                if !w.is_empty() {
                    r.findings.push(cx.at(df, rec.line, format!("{c}: {w}")));
                }
            }
        }
    });
    r
}

/// `forge_docker_ignore`: each Dockerfile has a .dockerignore (`<Dockerfile>.dockerignore`, the
/// one in its directory, or the one at the root) that excludes .env files.
pub fn ignore(cx: &Cx<'_>, _args: &[String]) -> Report {
    re!(ENV, r"^\s*((\*\*/)?\.env(\*|\.\*)?|\*\.env|\*|\*\*)\s*$");
    let mut r = Report::default();
    for df in dockerfiles(cx) {
        if !cx.exists(&df) {
            continue;
        }
        let d = dir_of(&df);
        let found = [
            format!("{df}.dockerignore"),
            join(&d, ".dockerignore"),
            ".dockerignore".to_string(),
        ]
        .into_iter()
        .find(|c| cx.exists(c));
        match found {
            None => r.findings.push(cx.at(
                &df,
                0,
                "has no .dockerignore (next to it or at the root); add one that excludes .env files",
            )),
            Some(ig) => {
                let text = cx.now(&ig).unwrap_or_default();
                if !text.lines().any(|l| ENV.is_match(l)) {
                    r.findings.push(cx.at(
                        &df,
                        0,
                        format!("{ig} does not exclude .env files; add .env* to it"),
                    ));
                }
            }
        }
    }
    r
}

/// `forge_docker_copy_order`: in a stage, a RUN that installs dependencies after a COPY or ADD of
/// the whole build context (`.` or `./`): every code change then reinstalls them.
pub fn copy_order(cx: &Cx<'_>, _args: &[String]) -> Report {
    re!(
        INSTALL,
        r"^(npm (ci|install|i)|bun (install|i)|uv sync|pnpm (install|i)|yarn( install)?|pip3? install( .*)? (-r|--requirement)|uv pip install( .*)? (-r|--requirement))( |$)"
    );
    re!(GLOBAL, r"^npm (install|i) .*(-g|--global)");
    let mut r = Report::default();
    each_dockerfile(cx, |df, text| {
        let mut all = false;
        for rec in records(text) {
            let instr = rec.instr();
            match instr.as_str() {
                "FROM" => all = false,
                "COPY" | "ADD" => {
                    let a = strip_flags(&rec.args());
                    let w = words(&a);
                    if w.len() > 1 && w[..w.len() - 1].iter().any(|x| *x == "." || *x == "./") {
                        all = true;
                    }
                }
                "RUN" if all => {
                    let pip = commands(&rec.args())
                        .iter()
                        .find(|c| INSTALL.is_match(c) && !GLOBAL.is_match(c))
                        .map(|c| c.contains("pip "));
                    if let Some(pip) = pip {
                        let files = if pip {
                            "the requirements files"
                        } else {
                            "the manifest and lock file"
                        };
                        r.findings.push(cx.at(
                            df,
                            rec.line,
                            format!(
                                "dependencies are installed after the whole build context is copied, so any code change reinstalls them; copy {files}, install, then COPY . ."
                            ),
                        ));
                    }
                }
                _ => {}
            }
        }
    });
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_join_continuations_and_drop_comments() {
        let r = records("# c\nFROM a\n\nRUN x \\\n  # note\n  && y\nUSER app\n");
        let t: Vec<(u32, &str)> = r.iter().map(|r| (r.line, r.text.as_str())).collect();
        assert_eq!(t, [(2, "FROM a"), (4, "RUN x    && y"), (7, "USER app")]);
        assert_eq!(r[1].instr(), "RUN");
        assert_eq!(r[1].args(), "x    && y");
    }

    #[test]
    fn commands_drop_flags_assignments_and_sudo() {
        assert_eq!(
            commands("--mount=type=cache,target=/x CI=1 sudo npm  install && npm run build"),
            ["npm install", "npm run build"]
        );
    }
}
