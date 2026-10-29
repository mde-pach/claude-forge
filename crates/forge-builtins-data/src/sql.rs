//! Databases and migrations (`sql.sh`): sqlfluff, Squawk, Alembic, Prisma, Drizzle, TypeORM,
//! applied migrations, mapped models a migration would miss.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::process::{Command, Stdio};

use forge_core::{NoteKind, Report};

use crate::Run;
use crate::tools::ToolRun;
use crate::util::{basename, dirname, join, last_line, re};

// ---- sqlfluff

/// `forge_sqlfluff [file...]`: when the project configures sqlfluff (.sqlfluff, [tool.sqlfluff]
/// in pyproject.toml, [sqlfluff] in setup.cfg or tox.ini), the project's sqlfluff lint on the
/// given .sql files (the whole project with `--all`).
pub fn sqlfluff(run: &Run<'_>, args: &[String]) -> Report {
    let configured = run.exists(".sqlfluff")
        || run.read("pyproject.toml").is_some_and(|t| {
            t.lines()
                .any(|l| re(r"^\[tool\.sqlfluff(\]|\.)").is_match(l))
        })
        || ["setup.cfg", "tox.ini"].iter().any(|f| {
            run.read(f)
                .is_some_and(|t| t.lines().any(|l| re(r"^\[sqlfluff(\]|:)").is_match(l)))
        });
    if !configured {
        return Report::default();
    }
    let mut files: Vec<String> = args
        .iter()
        .filter(|f| f.ends_with(".sql") && run.exists(f))
        .cloned()
        .collect();
    if files.is_empty() {
        if !run.all() {
            return Report::default();
        }
        files.push(".".into());
    }
    let out = match run
        .opts
        .project_tools
        .run(run.root(), "sqlfluff", "lint", &files)
    {
        ToolRun::Ran(o) => o,
        ToolRun::Missing => {
            return run.only_note(
                NoteKind::MissingTool,
                "sqlfluff is not installed, so the SQL files are not linted",
            );
        }
        ToolRun::Unsupported(why) => return run.only_note(NoteKind::NotEnforced, why),
    };
    match out.status {
        0 => return Report::default(),
        1 => {}
        _ => {
            return run.only_note(
                NoteKind::ForgeFailure,
                format!("sqlfluff could not run ({})", last_line(&out.text())),
            );
        }
    }
    let mut r = Report::default();
    let ann = re(r"^::(error|warning|notice) (.*)$");
    for l in out.text().lines() {
        let Some(c) = ann.captures(l) else { continue };
        let meta_all = &c[2];
        let (meta, msg) = meta_all.split_once("::").unwrap_or((meta_all, ""));
        let field = |k: &str| {
            meta.split(',')
                .find_map(|p| p.trim().strip_prefix(k).map(str::to_owned))
        };
        let Some(f) = field("file=") else { continue };
        let f = f.strip_prefix("./").unwrap_or(&f).to_owned();
        let line = field("line=").and_then(|l| l.parse().ok());
        r.findings.push(run.at(&f, line, msg));
    }
    if r.findings.is_empty() {
        r.notes.push(run.note(
            NoteKind::ForgeFailure,
            format!(
                "sqlfluff reported a failure Forge could not read ({})",
                last_line(&out.text())
            ),
        ));
    }
    r
}

// ---- Squawk

/// Squawk's rules whose findings break the running version of the application during a deploy;
/// Squawk's other advice is not enforced.
const SQUAWK_BLOCKING: &[&str] = &[
    "renaming-column",
    "renaming-table",
    "changing-column-type",
    "adding-required-field",
    "adding-not-nullable-field",
    "ban-drop-database",
];

/// What Squawk lints: repository files, or SQL reported as one path.
enum SquawkInput<'a> {
    Files(&'a [String]),
    Stdin { path: &'a str, sql: &'a str },
}

/// One blocking Squawk finding (lines and columns from 1).
struct SquawkHit {
    path: String,
    line: u32,
    col: u32,
    rule: String,
    message: String,
}

/// `forge_squawk`: the pinned Squawk with Forge's own config (an empty one: the project's
/// excluded_rules and excluded_paths do not apply), on copies of the SQL whose `squawk-ignore`
/// comments are made plain comments of the same length, run from the copies' directory so the
/// reported paths are the repository's. Err: why it could not run (for the person).
fn squawk(run: &Run<'_>, input: SquawkInput<'_>) -> Result<Vec<SquawkHit>, String> {
    let cmd = run.opts.pinned.command("squawk")?;
    let dir = tempfile::tempdir().map_err(|e| format!("squawk could not run ({e})"))?;
    let src = dir.path().join("src");
    let toml = dir.path().join("squawk.toml");
    std::fs::create_dir_all(&src).map_err(|e| e.to_string())?;
    std::fs::write(&toml, "").map_err(|e| e.to_string())?;
    let unignore = |s: &str| s.replace("squawk-ignore", "squawk-IGNORE");
    let mut c = Command::new(&cmd[0]);
    c.args(&cmd[1..])
        .args(["--reporter", "gcc", "-c"])
        .arg(&toml)
        .current_dir(&src)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut stdin = None;
    match input {
        SquawkInput::Files(files) => {
            for f in files {
                let text = std::fs::read(run.path(f)).map_err(|e| format!("{f}: {e}"))?;
                let to = src.join(f);
                if let Some(p) = to.parent() {
                    std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
                }
                std::fs::write(&to, unignore(&String::from_utf8_lossy(&text)))
                    .map_err(|e| e.to_string())?;
            }
            c.args(files).stdin(Stdio::null());
        }
        SquawkInput::Stdin { path, sql } => {
            c.args(["--stdin-filepath", path]).stdin(Stdio::piped());
            stdin = Some(unignore(sql));
        }
    }
    let mut child = c
        .spawn()
        .map_err(|e| format!("squawk could not run ({e})"))?;
    if let (Some(s), Some(mut w)) = (stdin, child.stdin.take()) {
        let _ = w.write_all(s.as_bytes());
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("squawk could not run ({e})"))?;
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    let line_re = re(r"^(.*):([0-9]+):([0-9]+): (warning|error): (\S+) (.*)$");
    // Squawk exits 1 whenever it warns, which is almost always
    if !out.status.success() && !text.lines().any(|l| line_re.is_match(l)) {
        return Err(format!("squawk could not run ({})", last_line(&text)));
    }
    Ok(text
        .lines()
        .filter_map(|l| line_re.captures(l))
        .filter(|c| SQUAWK_BLOCKING.contains(&&c[5]))
        .map(|c| SquawkHit {
            path: c[1].to_owned(),
            line: c[2].parse::<u32>().unwrap_or(0) + 1,
            col: c[3].parse::<u32>().unwrap_or(0) + 1,
            rule: c[5].to_owned(),
            message: c[6].trim().to_owned(),
        })
        .collect())
}

/// A Squawk that is not installed, or one that could not run.
fn squawk_kind(why: &str) -> NoteKind {
    if why.contains("not installed") {
        NoteKind::MissingTool
    } else {
        NoteKind::ForgeFailure
    }
}

/// `forge_squawk_files <sql files>`: lints migration SQL files.
fn squawk_on(run: &Run<'_>, files: &[String]) -> Report {
    if files.is_empty() {
        return Report::default();
    }
    match squawk(run, SquawkInput::Files(files)) {
        Err(why) => run.only_note(squawk_kind(&why), why),
        Ok(hits) => Report {
            findings: hits
                .into_iter()
                .map(|h| {
                    run.at(
                        &h.path,
                        Some(h.line),
                        format!("{}: {} (column {})", h.rule, h.message, h.col),
                    )
                })
                .collect(),
            notes: vec![],
        },
    }
}

pub fn squawk_files(run: &Run<'_>, args: &[String]) -> Report {
    let files: Vec<String> = args.iter().filter(|f| !f.is_empty()).cloned().collect();
    squawk_on(run, &files)
}

/// `forge_sql_postgres <file>`: the plain-SQL migration tool that owns the file targets
/// Postgres (Squawk lints Postgres only), from literal config only.
fn sql_postgres(run: &Run<'_>, file: &str) -> bool {
    if file.starts_with("supabase/migrations/") {
        return true;
    }
    if run.exists("flyway.conf") || run.exists("conf/flyway.conf") || run.exists("flyway.toml") {
        let conf = run.read("flyway.conf").unwrap_or_default()
            + &run.read("conf/flyway.conf").unwrap_or_default();
        if conf
            .lines()
            .any(|l| re(r"^[ \t]*flyway\.url[ \t]*=[ \t]*jdbc:postgresql:").is_match(l))
        {
            return true;
        }
        return run.read("flyway.toml").is_some_and(|t| {
            t.lines()
                .any(|l| re(r#"^[ \t]*url[ \t]*=[ \t]*["']jdbc:postgresql:"#).is_match(l))
        });
    }
    if run.exists("sqitch.plan") {
        return run.read("sqitch.conf").is_some_and(|t| {
            t.lines()
                .any(|l| re(r"^[ \t]*engine[ \t]*=[ \t]*pg[ \t]*$").is_match(l))
        });
    }
    if let Some(atlas) = run.read("atlas.hcl") {
        return atlas.lines().any(|l| {
            re(r#"^[ \t]*(url|dev)[ \t]*=[ \t]*"(postgres(ql)?://|docker://postgres)"#).is_match(l)
        });
    }
    let url = run.db_url();
    url.starts_with("postgres://") || url.starts_with("postgresql://")
}

/// `forge_squawk_role <@role> [file...]`: Squawk on the given files of the role that target
/// Postgres; with none given (`--all`), on the role's newest file by name, so an accepted
/// change does not fail every later run.
pub fn squawk_role(run: &Run<'_>, args: &[String]) -> Report {
    let Some(role) = args.first() else {
        return Report::default();
    };
    let mut files: Vec<String> = args[1..].to_vec();
    if files.is_empty() {
        let mut all = run.role_files(role);
        all.sort();
        files.extend(all.pop());
    }
    let files: Vec<String> = files
        .into_iter()
        .filter(|f| run.exists(f) && run.role(role, f) && sql_postgres(run, f))
        .collect();
    squawk_on(run, &files)
}

// ---- Alembic

fn revision_of(text: &str) -> Option<String> {
    let r = re(r#"^revision[^=]*=[ \t]*['"]([^'"]*)['"]"#);
    text.lines()
        .find_map(|l| r.captures(l).map(|c| c[1].to_owned()))
}

/// `forge_alembic_files`: Alembic revision ids on stdin (as `alembic heads` prints them) ->
/// the revision files (file role @alembic-migrations) that define them, one per line.
pub fn alembic_files(run: &Run<'_>, _args: &[String]) -> Report {
    let revs: Vec<String> = run
        .role_files("@alembic-migrations")
        .into_iter()
        .filter(|f| f.ends_with(".py"))
        .collect();
    let mut r = Report::default();
    if revs.is_empty() {
        return r;
    }
    let texts: Vec<(String, String)> = revs
        .iter()
        .filter_map(|f| run.read(f).map(|t| (f.clone(), t)))
        .collect();
    for line in run.stdin().lines() {
        let Some(id) = line.split_whitespace().next() else {
            continue;
        };
        let pat = format!(r#"^revision[^=]*=[ \t]*['"]{}['"]"#, regex::escape(id));
        let def = re(&pat);
        for (f, t) in &texts {
            if t.lines().any(|l| def.is_match(l)) {
                r.findings.push(run.finding(f.clone()));
            }
        }
    }
    r
}

/// `forge_squawk_alembic <revision files>`: lints the SQL each revision's upgrade emits
/// (`alembic upgrade <down_revision>:<revision> --sql`, offline). Merge revisions are left out.
pub fn squawk_alembic(run: &Run<'_>, args: &[String]) -> Report {
    let mut r = Report::default();
    for f in args.iter().filter(|f| !f.is_empty()) {
        let Some(text) = run.read(f) else { continue };
        let Some(rev) = revision_of(&text) else {
            continue;
        };
        let down_re = re(r"^down_revision[^=]*=[ \t]*(.*)$");
        let down = text
            .lines()
            .find_map(|l| down_re.captures(l).map(|c| c[1].to_owned()))
            .unwrap_or_default();
        if down.starts_with('(') || down.starts_with('[') {
            continue;
        }
        let range = if down.is_empty() || down.starts_with("None") {
            rev.clone()
        } else {
            let d = re(r#"^['"]([^'"]*)['"].*$"#)
                .replace(&down, "$1")
                .into_owned();
            format!("{d}:{rev}")
        };
        let out = match run.opts.project_tools.run(
            run.root(),
            "alembic",
            "upgrade-sql",
            std::slice::from_ref(&range),
        ) {
            ToolRun::Ran(o) => o,
            ToolRun::Missing => {
                r.notes.push(run.note(
                    NoteKind::MissingTool,
                    "alembic is not installed, so the revisions' SQL is not linted",
                ));
                return r;
            }
            ToolRun::Unsupported(why) => {
                r.notes.push(run.note(NoteKind::NotEnforced, why));
                return r;
            }
        };
        if out.status != 0 {
            r.notes.push(run.note(
                NoteKind::Environment,
                format!(
                    "{f} is not linted: alembic upgrade {range} --sql failed ({})",
                    last_line(&out.stderr)
                ),
            ));
            continue;
        }
        let hits = match squawk(
            run,
            SquawkInput::Stdin {
                path: f,
                sql: &out.stdout,
            },
        ) {
            Ok(h) => h,
            Err(why) => {
                r.notes.push(run.note(squawk_kind(&why), why));
                return r;
            }
        };
        if hits.is_empty() {
            continue;
        }
        let mut msg = format!(
            "the SQL that alembic upgrade {range} --sql renders breaks the running app during a deploy (line numbers are in that SQL):"
        );
        for h in hits {
            msg.push_str(&format!("\n  line {}: {}: {}", h.line, h.rule, h.message));
        }
        r.findings.push(run.at(f, None, msg));
    }
    r
}

/// The quoted ids of a line's value (`["a", "b"]`, `"a"`).
fn quoted_ids(s: &str) -> Vec<String> {
    re(r#"["']([^"']+)["']"#)
        .captures_iter(s)
        .map(|c| c[1].to_owned())
        .collect()
}

/// `forge_alembic_targets`: the revision files to examine: the in-scope changed ones, else
/// (`--all`) the head revisions' (no file names them as a down_revision), so revisions written
/// before a rule was adopted do not fail CI.
fn alembic_targets(run: &Run<'_>) -> Vec<String> {
    if !run.files().is_empty() {
        return run
            .files()
            .iter()
            .filter(|f| f.ends_with(".py") && run.role("@alembic-migrations", f))
            .cloned()
            .collect();
    }
    let mut revs: Vec<String> = run
        .role_files("@alembic-migrations")
        .into_iter()
        .filter(|f| f.ends_with(".py"))
        .collect();
    revs.sort();
    let mut rev: BTreeMap<String, String> = BTreeMap::new();
    let mut down: HashSet<String> = HashSet::new();
    for f in &revs {
        let Some(t) = run.read(f) else { continue };
        for l in t.lines() {
            if let Some(v) = re(r"^revision[^=]*=(.*)$").captures(l) {
                if let Some(id) = quoted_ids(&v[1]).into_iter().next() {
                    rev.insert(f.clone(), id);
                }
            } else if let Some(v) = re(r"^down_revision[^=]*=(.*)$").captures(l) {
                down.extend(quoted_ids(&v[1]));
            }
        }
    }
    rev.into_iter()
        .filter(|(_, id)| !down.contains(id))
        .map(|(f, _)| f)
        .collect()
}

/// The project also runs its migrations on SQLite: alembic/env.py sets render_as_batch=True,
/// or alembic.ini's sqlalchemy.url is a sqlite URL.
fn alembic_sqlite(run: &Run<'_>) -> bool {
    run.read("alembic/env.py").is_some_and(|t| {
        t.lines()
            .any(|l| re(r"^[^#]*render_as_batch[ \t]*=[ \t]*True").is_match(l))
    }) || run.read("alembic.ini").is_some_and(|t| {
        t.lines()
            .any(|l| re(r"^[ \t]*sqlalchemy\.url[ \t]*=[ \t]*sqlite").is_match(l))
    })
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

/// `forge_alembic_batch`: on a project that runs its migrations on SQLite too, operations
/// SQLite cannot run as ALTER TABLE go through op.batch_alter_table(). Operations under an
/// `if` on the dialect are left alone.
pub fn alembic_batch(run: &Run<'_>, _args: &[String]) -> Report {
    let mut r = Report::default();
    if !alembic_sqlite(run) {
        return r;
    }
    let op_re = re(
        r"(?:^|[^A-Za-z0-9_.])op\.(alter_column|drop_column|create_foreign_key|drop_constraint|create_unique_constraint|create_check_constraint|create_primary_key)\(",
    );
    for f in alembic_targets(run) {
        let Some(text) = run.read(&f) else { continue };
        let mut skip: Option<usize> = None;
        for (i, raw) in text.lines().enumerate() {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            if re(r"^[ \t]*(#|$)").is_match(line) {
                continue;
            }
            let ind = indent_of(line);
            if let Some(s) = skip {
                if ind > s {
                    continue;
                }
                if ind == s && re(r"^[ \t]*(elif|else)[ \t:]").is_match(line) {
                    continue;
                }
                skip = None;
            }
            if re(r"^[ \t]*(if|elif)[ \t].*(dialect|sqlite|postgres|mysql)").is_match(line) {
                skip = Some(ind);
                continue;
            }
            if let Some(c) = op_re.captures(line) {
                let op = &c[1];
                r.findings.push(run.at(
                    &f,
                    Some(i as u32 + 1),
                    format!(
                        "op.{op}() outside op.batch_alter_table(): SQLite cannot ALTER a table this way, and this project runs its migrations on SQLite too (render_as_batch=True or a sqlite URL). Use: with op.batch_alter_table(\"<table>\") as batch_op: batch_op.{op}(...)"
                    ),
                ));
            }
        }
    }
    r
}

/// The position of the bracket closing the one at `p` (strings skipped); the end otherwise.
fn close_of(s: &[u8], p: usize) -> usize {
    let mut d = 0i32;
    let mut q: Option<u8> = None;
    let mut i = p;
    while i < s.len() {
        let c = s[i];
        if let Some(qc) = q {
            if c == b'\\' {
                i += 1;
            } else if c == qc {
                q = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' => q = Some(c),
            b'(' | b'[' => d += 1,
            b')' | b']' => {
                d -= 1;
                if d == 0 {
                    return i;
                }
            }
            _ => {}
        }
        i += 1;
    }
    s.len().saturating_sub(1)
}

/// The quoted strings of `t` (the first only unless `all`).
fn strings(t: &str, all: bool) -> Vec<String> {
    let mut v: Vec<String> = re(r#"["']([^"']*)["']"#)
        .captures_iter(t)
        .map(|c| c[1].to_owned())
        .collect();
    if !all {
        v.truncate(1);
    }
    v
}

/// The first `[...]` of `t`.
fn first_list(t: &str) -> &str {
    let Some(p) = t.find('[') else { return "" };
    let rest = &t[p..];
    let end = close_of(rest.as_bytes(), 0);
    &rest[..=end.min(rest.len() - 1)]
}

/// The body of `def <name>(...)` at the top level: its lines (comment lines blanked) and the
/// line number of the first.
fn function_body(text: &str, name: &str) -> Option<(Vec<String>, usize)> {
    let head = re(&format!(r"^def[ \t]+{name}[ \t]*\("));
    let mut lines = text.lines().enumerate();
    let first = lines.find(|(_, l)| head.is_match(l))?.0 + 2;
    let mut body = vec![];
    for (_, raw) in lines {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if re(r"^[^ \t#]").is_match(line) {
            break;
        }
        body.push(if re(r"^[ \t]*#").is_match(line) {
            String::new()
        } else {
            line.to_owned()
        });
    }
    Some((body, first))
}

/// `forge_alembic_fk_index`: in each revision's upgrade(), a column that gets a foreign key
/// also leads an index; skipped when the database URL is MySQL (which indexes them).
pub fn alembic_fk_index(run: &Run<'_>, _args: &[String]) -> Report {
    let mut r = Report::default();
    let url = run.db_url();
    if url.starts_with("mysql") || url.starts_with("mariadb") {
        return r;
    }
    for f in alembic_targets(run) {
        let Some(text) = run.read(&f) else { continue };
        let Some((body, first)) = function_body(&text, "upgrade") else {
            continue;
        };
        let mut s = String::new();
        let mut starts = vec![];
        for l in &body {
            starts.push(s.len());
            s.push_str(l);
            s.push('\n');
        }
        let line_of = |p: usize| -> u32 {
            let k = starts.iter().rposition(|&st| st <= p).unwrap_or(0);
            (first + k) as u32
        };
        let bytes = s.as_bytes();
        let mut fk: Vec<(String, u32)> = vec![];
        let mut cov: HashSet<String> = HashSet::new();
        let call = re(
            r"(?:^|[^A-Za-z0-9_])(Column|ForeignKeyConstraint|create_foreign_key|create_index|PrimaryKeyConstraint|UniqueConstraint|Index)\(",
        );
        let mut off = 0;
        while let Some(c) = call.captures(&s[off..]) {
            let name = c[1].to_owned();
            let po = off + c.get(0).map_or(0, |m| m.end()) - 1;
            let pc = close_of(bytes, po);
            let args = &s[po + 1..pc.max(po + 1)];
            let add_fk = |fk: &mut Vec<(String, u32)>, col: String| {
                if !col.is_empty() && !fk.iter().any(|(c, _)| *c == col) {
                    fk.push((col, line_of(po)));
                }
            };
            match name.as_str() {
                "Column" => {
                    if re(r#"^[ \t\n]*["']"#).is_match(args) {
                        let col = strings(args, false)
                            .pop()
                            .unwrap_or_default()
                            .to_lowercase();
                        if re(r"(?:^|[^A-Za-z0-9_])ForeignKey\(").is_match(args) {
                            add_fk(&mut fk, col.clone());
                        }
                        if re(r"(index|primary_key|unique)[ \t]*=[ \t]*True").is_match(args) {
                            cov.insert(col);
                        }
                    }
                }
                "ForeignKeyConstraint" | "create_foreign_key" => {
                    let col = strings(first_list(args), false)
                        .pop()
                        .unwrap_or_default()
                        .to_lowercase();
                    add_fk(&mut fk, col);
                }
                "create_index" => cov.extend(
                    strings(first_list(args), false)
                        .into_iter()
                        .map(|c| c.to_lowercase()),
                ),
                "Index" => cov.extend(strings(args, true).get(1).map(|c| c.to_lowercase())),
                _ => cov.extend(strings(args, false).into_iter().map(|c| c.to_lowercase())),
            }
            off = po + 1;
        }
        let u = s.to_uppercase();
        let ci = re(r"CREATE[ \t\n]+(?:UNIQUE[ \t\n]+)?INDEX[^(]*\([ \t\n]*");
        for m in ci.find_iter(&u) {
            let t: String = u[m.end()..]
                .chars()
                .take_while(|c| {
                    c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_' || *c == '"'
                })
                .filter(|c| *c != '"')
                .collect();
            cov.insert(t.to_lowercase());
        }
        for (col, line) in fk {
            if cov.contains(&col) {
                continue;
            }
            r.findings.push(run.at(
                &f,
                Some(line),
                format!(
                    "foreign key column \"{col}\" gets no index that leads with it in this revision; PostgreSQL and SQLite do not index referencing columns, so joins on it and deletes of the referenced rows scan the table. If it is queried or the parent rows are deleted, add op.create_index(..., [\"{col}\"]) (index=True on the model mapped_column, then autogenerate); otherwise say why not."
                ),
            ));
        }
    }
    r
}

fn changes_rows(t: &str) -> bool {
    let u = t.to_uppercase();
    (t.contains("execute(")
        && (re(r"(?:^|[^A-Z_])UPDATE[ \t\n]+[^ \t\n]+[ \t\n]+SET[ \t\n]").is_match(&u)
            || re(r"INSERT[ \t\n]+INTO[ \t\n]").is_match(&u)
            || re(r"DELETE[ \t\n]+FROM[ \t\n]").is_match(&u)))
        || re(r"(?:^|[^A-Za-z0-9_.])(?:sa\.)?(?:update|insert|delete)\([A-Za-z_]").is_match(t)
        || re(r"\.(?:update|insert|delete)\(\)").is_match(t)
        || t.contains("op.bulk_insert(")
        || re(r"(?:^|[^A-Za-z0-9_])Session\(").is_match(t)
}

/// `forge_alembic_data_downgrade`: a revision whose upgrade() changes rows has a downgrade()
/// that changes rows (the inverse) or raises.
pub fn alembic_data_downgrade(run: &Run<'_>, _args: &[String]) -> Report {
    let mut r = Report::default();
    for f in alembic_targets(run) {
        let Some(text) = run.read(&f) else { continue };
        let (mut up, mut down) = (String::new(), String::new());
        let mut sec = "";
        let mut dl = 0usize;
        let mut raises = false;
        let mut indoc: Option<&str> = None;
        for (i, raw) in text.lines().enumerate() {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            if re(r"^def[ \t]+upgrade[ \t]*\(").is_match(raw) {
                sec = "up";
                continue;
            }
            if re(r"^def[ \t]+downgrade[ \t]*\(").is_match(raw) {
                sec = "down";
                dl = i + 1;
                continue;
            }
            if re(r"^[^ \t#]").is_match(line) {
                sec = "";
                continue;
            }
            if sec == "up" && !re(r"^[ \t]*#").is_match(line) {
                up.push_str(line);
                up.push('\n');
            }
            if sec == "down" {
                let t = line.trim_matches([' ', '\t']);
                if let Some(q) = indoc {
                    if t.contains(q) {
                        indoc = None;
                    }
                    continue;
                }
                if t.is_empty() || t.starts_with('#') {
                    continue;
                }
                if let Some(q) = ["\"\"\"", "'''"].into_iter().find(|q| t.starts_with(q)) {
                    if !t[3..].contains(q) {
                        indoc = Some(q);
                    }
                    continue;
                }
                if re(r"^raise([ \t(]|$)").is_match(t) {
                    raises = true;
                }
                down.push_str(line);
                down.push('\n');
            }
        }
        if changes_rows(&up) && !raises && !changes_rows(&down) {
            r.findings.push(run.at(
                &f,
                Some(dl.max(1) as u32),
                "upgrade() changes rows but downgrade() neither changes them back (op.execute with UPDATE/INSERT/DELETE, sa.update/insert/delete, op.bulk_insert) nor raises, so alembic downgrade reports success and leaves the rows changed. Write the inverse, or raise an error that says the revision cannot be reverted.",
            ));
        }
    }
    r
}

/// `forge_mentions <text> <name>`: the name appears as a word outside comment lines.
fn mentions(text: &str, name: &str) -> bool {
    let w = re(&format!(
        r"(?:^|[^A-Za-z0-9_]){}(?:[^A-Za-z0-9_]|$)",
        regex::escape(name)
    ));
    text.lines()
        .filter(|l| !re(r"^[ \t]*#").is_match(l))
        .any(|l| w.is_match(l))
}

/// The modules an `__init__.py` imports (their last name component).
fn imported_modules(init: &str) -> Vec<String> {
    let mut out = vec![];
    for l in init.lines() {
        if re(r"^[ \t]*from[ \t]+\.[ \t]+import[ \t]").is_match(l) {
            let s = re(r"^.*import[ \t]+")
                .replace(l, "")
                .replace(['(', ')', ','], " ");
            out.extend(
                s.split_whitespace()
                    .filter(|w| *w != "as")
                    .map(str::to_owned),
            );
        } else if let Some(c) = re(r"^[ \t]*from[ \t]+([A-Za-z0-9_.]+)[ \t]+import").captures(l) {
            out.push(c[1].rsplit('.').next().unwrap_or("").to_owned());
        } else if let Some(c) = re(r"^[ \t]*import[ \t]+([A-Za-z0-9_.]+)").captures(l) {
            out.push(c[1].rsplit('.').next().unwrap_or("").to_owned());
        }
    }
    out
}

/// `forge_models_registered`: a module of a models package that maps a table is imported by
/// the package's `__init__.py` when that file is the registry (it imports other modules of the
/// package that map tables), or by alembic/env.py.
pub fn models_registered(run: &Run<'_>, _args: &[String]) -> Report {
    let files: Vec<String> = if !run.files().is_empty() {
        run.files().to_vec()
    } else {
        run.ls_files(&[
            "-co",
            "--exclude-standard",
            "--",
            "models/*.py",
            "*/models/*.py",
        ])
    };
    let mut r = Report::default();
    for f in files {
        let b = basename(&f);
        if b == "__init__.py" || b == "conftest.py" || b.starts_with("test_") || !b.ends_with(".py")
        {
            continue;
        }
        let Some(text) = run.read(&f) else { continue };
        let d = dirname(&f);
        let m = b.trim_end_matches(".py");
        let Some(init) = run.read(&join(&d, "__init__.py")) else {
            continue;
        };
        let mut classes = vec![];
        let mut cur: Option<(usize, String)> = None;
        for (i, l) in text.lines().enumerate() {
            if let Some(c) = re(r"^class[ \t]+([A-Za-z_][A-Za-z0-9_]*)").captures(l) {
                cur = Some((i + 1, c[1].to_owned()));
                continue;
            }
            if re(r"^[^ \t#\r]").is_match(l) {
                cur = None;
            }
            if cur.is_some() && re(r"^[ \t]+__tablename__[ \t]*[:=]").is_match(l) {
                classes.extend(cur.take());
            }
        }
        if classes.is_empty() || mentions(&init, m) {
            continue;
        }
        if run.read("alembic/env.py").is_some_and(|t| mentions(&t, m)) {
            continue;
        }
        let registry = imported_modules(&init).iter().any(|x| {
            x != m
                && run
                    .read(&join(&d, &format!("{x}.py")))
                    .is_some_and(|t| t.contains("__tablename__"))
        });
        if !registry {
            continue;
        }
        for (line, class) in classes {
            r.findings.push(run.at(
                &f,
                Some(line as u32),
                format!(
                    "model {class} is not imported by {}, which imports the other models: Base.metadata will not have its table, so autogenerate writes no migration for it and alembic check passes. Import it there.",
                    join(&d, "__init__.py")
                ),
            ));
        }
    }
    r
}

// ---- Prisma

/// A table's columns: upper-cased for comparison, as written for messages.
type Cols = Vec<(String, String)>;

fn unq(t: &str) -> String {
    t.chars()
        .filter(|c| !matches!(c, '"' | ';' | '`') && !c.is_whitespace())
        .collect()
}

/// The name right after the part of `stmt` that `prefix` (a regex) matches.
fn name_after(stmt: &str, prefix: &str) -> String {
    let p = re(prefix);
    let Some(m) = p.find(stmt) else {
        return String::new();
    };
    let rest = &stmt[m.end()..];
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '(')
        .unwrap_or(rest.len());
    unq(&rest[..end])
}

/// The column names of a CREATE TABLE body: the first identifier of each top-level item.
fn columns(stmt: &str) -> Cols {
    let Some(p) = stmt.find('(') else {
        return vec![];
    };
    let mut items = vec![];
    let mut item = String::new();
    let mut d = 0;
    for c in stmt[p + 1..].chars() {
        if c == '(' {
            d += 1;
        }
        if c == ')' {
            if d == 0 {
                break;
            }
            d -= 1;
        }
        if c == ',' && d == 0 {
            items.push(std::mem::take(&mut item));
            continue;
        }
        item.push(c);
    }
    items.push(item);
    items
        .iter()
        .filter_map(|it| {
            let t = it.trim_start();
            if re(r"^(?i:CONSTRAINT|PRIMARY|FOREIGN|UNIQUE|CHECK|KEY|INDEX)(?:[^A-Za-z_]|$)")
                .is_match(t)
            {
                return None;
            }
            let m = re(r#"^("[^"]+"|`[^`]+`|[A-Za-z_][A-Za-z0-9_]*)"#).find(t)?;
            let name = unq(m.as_str());
            Some((name.to_uppercase(), name))
        })
        .collect()
}

struct Dropped {
    cols: Cols,
    line: usize,
    text: String,
}

/// `forge_prisma_drops <file>...`: `<file>:<line>: <statement>` for each DROP COLUMN or DROP
/// TABLE in the given Prisma migrations. Every migration.sql (sorted) is read to follow the
/// tables' columns, so a SQLite table redefinition (CREATE TABLE "new_X", DROP TABLE "X",
/// ALTER TABLE "new_X" RENAME TO "X") is a drop only of the columns "new_X" lacks. A redefined
/// table that no earlier migration creates is not reported.
pub fn prisma_drops(run: &Run<'_>, args: &[String]) -> Report {
    let targets: HashSet<&str> = args.iter().map(String::as_str).collect();
    let mut history: BTreeSet<String> = run
        .ls_files(&[
            "-co",
            "--exclude-standard",
            "--",
            "prisma/migrations/*/migration.sql",
        ])
        .into_iter()
        .collect();
    history.extend(args.iter().filter(|a| !a.is_empty()).cloned());
    let mut out: Vec<String> = vec![];
    let mut cols: HashMap<String, Cols> = HashMap::new();
    for f in &history {
        let target = targets.contains(f.as_str());
        let mut pending: BTreeMap<String, Dropped> = BTreeMap::new();
        let text = run.read(f).unwrap_or_default();
        let mut stmt = String::new();
        let mut sline = 0;
        let mut first = String::new();
        let mut incomment = false;
        for (i, raw) in text.lines().enumerate() {
            let mut l = raw;
            if incomment {
                match l.find("*/") {
                    Some(p) => {
                        incomment = false;
                        l = &l[p + 2..];
                    }
                    None => continue,
                }
            }
            if re(r"^[ \t]*/\*").is_match(l) && !l.contains("*/") {
                incomment = true;
                continue;
            }
            if re(r"^[ \t]*--").is_match(l) {
                continue;
            }
            if stmt.is_empty() && l.trim().is_empty() {
                continue;
            }
            if stmt.is_empty() {
                sline = i + 1;
                first = l.trim_start().to_owned();
            }
            stmt.push(' ');
            stmt.push_str(l);
            if l.contains(';') {
                statement(
                    f,
                    target,
                    sline,
                    &first,
                    &std::mem::take(&mut stmt),
                    &mut cols,
                    &mut pending,
                    &mut out,
                );
            }
        }
        // a table dropped for a redefinition that never came: a plain drop
        if target {
            for d in pending.values() {
                out.push(format!("{f}:{}: {}", d.line, d.text));
            }
        }
    }
    Report {
        findings: out.into_iter().map(|l| run.finding(l)).collect(),
        notes: vec![],
    }
}

#[allow(clippy::too_many_arguments)]
fn statement(
    f: &str,
    target: bool,
    line: usize,
    text: &str,
    stmt: &str,
    cols: &mut HashMap<String, Cols>,
    pending: &mut BTreeMap<String, Dropped>,
    out: &mut Vec<String>,
) {
    let u = stmt.to_uppercase();
    if re(r"^[ \t]*CREATE[ \t]+TABLE").is_match(&u) {
        let t = name_after(
            &u,
            r"^[ \t]*CREATE[ \t]+TABLE[ \t]+(?:IF[ \t]+NOT[ \t]+EXISTS[ \t]+)?",
        );
        cols.insert(t, columns(stmt));
        return;
    }
    if re(r"^[ \t]*ALTER[ \t]+TABLE").is_match(&u) {
        let t = name_after(
            &u,
            r"^[ \t]*ALTER[ \t]+TABLE[ \t]+(?:ONLY[ \t]+)?(?:IF[ \t]+EXISTS[ \t]+)?",
        );
        if re(r"RENAME[ \t]+TO[ \t]").is_match(&u) {
            let n = name_after(&u, r".*RENAME[ \t]+TO[ \t]+");
            if let Some(d) = pending.remove(&n) {
                let now = cols.get(&t).cloned().unwrap_or_default();
                let gone: Vec<String> = d
                    .cols
                    .iter()
                    .filter(|(k, _)| !now.iter().any(|(c, _)| c == k))
                    .map(|(_, shown)| format!("\"{shown}\""))
                    .collect();
                if target && !gone.is_empty() {
                    out.push(format!(
                        "{f}:{}: {} (redefinition drops column {})",
                        d.line,
                        d.text,
                        gone.join(", ")
                    ));
                }
            }
            let c = cols.get(&t).cloned().unwrap_or_default();
            cols.insert(n, c);
            return;
        }
        let entry = cols.entry(t).or_default();
        if re(r"RENAME[ \t]+COLUMN").is_match(&u) {
            let a = name_after(&u, r".*RENAME[ \t]+COLUMN[ \t]+");
            let b = name_after(&u, r".*[ \t]TO[ \t]+");
            entry.retain(|(k, _)| *k != a);
            entry.push((b.clone(), b));
            return;
        }
        let add = re(r#"ADD[ \t]+(?:COLUMN[ \t]+)?("[^"]+"|[A-Z_][A-Z0-9_]*)"#);
        for c in add.captures_iter(&u) {
            let start = c.get(1).map_or(0, |m| m.start());
            let k = unq(&c[1]);
            if matches!(
                k.as_str(),
                "CONSTRAINT" | "PRIMARY" | "FOREIGN" | "UNIQUE" | "CHECK"
            ) {
                continue;
            }
            let shown = unq(stmt.get(start..start + c[1].len()).unwrap_or(&c[1]));
            entry.push((k, shown));
        }
        let drop =
            re(r#"DROP[ \t]+COLUMN[ \t]+(?:IF[ \t]+EXISTS[ \t]+)?("[^"]+"|[A-Z_][A-Z0-9_]*)"#);
        for c in drop.captures_iter(&u) {
            let k = unq(&c[1]);
            entry.retain(|(x, _)| *x != k);
        }
        if target && re(r"DROP[ \t]+COLUMN").is_match(&u) {
            out.push(format!("{f}:{line}: {text}"));
        }
        return;
    }
    if re(r"^[ \t]*DROP[ \t]+TABLE").is_match(&u) {
        let t = name_after(&u, r"^[ \t]*DROP[ \t]+TABLE[ \t]+(?:IF[ \t]+EXISTS[ \t]+)?");
        // a redefinition renames "new_<t>" to <t> later in the same file (its old columns
        // unknown without the earlier migrations: not reported)
        if cols.contains_key(&format!("NEW_{t}")) {
            if let Some(c) = cols.get(&t) {
                pending.insert(
                    t.clone(),
                    Dropped {
                        cols: c.clone(),
                        line,
                        text: text.to_owned(),
                    },
                );
            }
        } else if target {
            out.push(format!("{f}:{line}: {text}"));
        }
        cols.remove(&t);
    }
}

// ---- Drizzle

/// drizzle.config.*: the schema entries, the migrations folder (`out`, default ./drizzle) and
/// the dialect.
struct DrizzleConfig {
    file: String,
    out: String,
    dialect: String,
    schema: Vec<String>,
}

fn drizzle_config(run: &Run<'_>) -> Option<DrizzleConfig> {
    let file = [
        "drizzle.config.ts",
        "drizzle.config.mts",
        "drizzle.config.cts",
        "drizzle.config.js",
        "drizzle.config.mjs",
        "drizzle.config.cjs",
        "drizzle.config.json",
    ]
    .into_iter()
    .find(|f| run.exists(f))?;
    let src = run.read(file).unwrap_or_default();
    // comments dropped (outside strings), newlines made spaces
    let b: Vec<char> = src.chars().collect();
    let mut t = String::new();
    let mut q: Option<char> = None;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if let Some(qc) = q {
            t.push(c);
            if c == '\\' {
                if let Some(n) = b.get(i + 1) {
                    t.push(*n);
                }
                i += 1;
            } else if c == qc {
                q = None;
            }
            i += 1;
            continue;
        }
        if c == '/' && b.get(i + 1) == Some(&'/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            t.push(' ');
            continue;
        }
        if c == '/' && b.get(i + 1) == Some(&'*') {
            i += 2;
            while i < b.len() && !(b[i] == '*' && b.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
            t.push(' ');
            continue;
        }
        if matches!(c, '"' | '\'' | '`') {
            q = Some(c);
        }
        t.push(if c == '\n' { ' ' } else { c });
        i += 1;
    }
    let s = format!(
        " {}",
        re(r"migrations[ \t\n]*:[ \t\n]*\{[^}]*\}").replace_all(&t, "")
    );
    let val = |key: &str| -> String {
        let p =
            format!(r#"[^A-Za-z0-9_$]"?{key}"?[ \t]*:[ \t]*(\[[^\]]*\]|"[^"]*"|'[^']*'|`[^`]*`)"#);
        re(&p)
            .captures(&s)
            .map(|c| c[1].to_owned())
            .unwrap_or_default()
    };
    let emit = |v: &str| -> Vec<String> {
        re(r#""([^"]*)"|'([^']*)'|`([^`]*)`"#)
            .captures_iter(v)
            .filter_map(|c| {
                let q = c.get(1).or(c.get(2)).or(c.get(3))?.as_str();
                let q = q.strip_prefix("./").unwrap_or(q).trim_end_matches('/');
                (!q.is_empty()).then(|| q.to_owned())
            })
            .collect()
    };
    let out_v = val("out");
    let out = if out_v.is_empty() {
        "drizzle".to_owned()
    } else {
        emit(&out_v).into_iter().next().unwrap_or_default()
    };
    Some(DrizzleConfig {
        file: file.to_owned(),
        out,
        dialect: emit(&val("dialect")).into_iter().next().unwrap_or_default(),
        schema: emit(&val("schema")),
    })
}

impl DrizzleConfig {
    /// The path is a schema entry, matches one as a glob, or sits under one as a directory.
    fn is_schema(&self, path: &str) -> bool {
        self.schema.iter().any(|v| {
            path == v
                || path.starts_with(&format!("{v}/"))
                || crate::glob_matches(&v.replace("**", "*"), path)
        })
    }

    fn under(&self, path: &str, pattern: &str) -> bool {
        crate::glob_matches(&format!("{}/{pattern}", glob_escape(&self.out)), path)
    }
}

fn glob_escape(s: &str) -> String {
    s.chars()
        .map(|c| {
            if matches!(c, '*' | '?' | '[' | ']' | '{' | '}') {
                format!("[{c}]")
            } else {
                c.to_string()
            }
        })
        .collect()
}

/// A schema file without what does not reach the database (blank lines, comments, imports,
/// type exports, relations()/defineRelations() calls).
fn drizzle_tables(text: Option<String>) -> Vec<String> {
    let Some(text) = text else { return vec![] };
    let mut out = vec![];
    let (mut inc, mut inrel, mut depth) = (false, false, 0i64);
    for l in text.lines() {
        if inc {
            if l.contains("*/") {
                inc = false;
            }
            continue;
        }
        if re(r"^[ \t]*/\*").is_match(l) {
            if !l.contains("*/") {
                inc = true;
            }
            continue;
        }
        if re(r"^[ \t]*(//|$)").is_match(l) {
            continue;
        }
        if re(r"^[ \t]*(import[ \t{*]|export[ \t]+(type|interface)[ \t]|export[ \t]*\*)")
            .is_match(l)
        {
            continue;
        }
        if depth == 0 && re(r"(?:^|[^A-Za-z0-9_$])(defineRelations|relations)[ \t]*\(").is_match(l)
        {
            inrel = true;
        }
        if inrel {
            depth += l.matches('(').count() as i64 - l.matches(')').count() as i64;
            if depth <= 0 {
                depth = 0;
                inrel = false;
            }
            continue;
        }
        out.push(l.trim().to_owned());
    }
    out
}

/// `forge_drizzle_migration_present`: a change to a schema file that reaches the database comes
/// with a generated migration in the out folder: the SQL plus meta/_journal.json and a
/// meta/*_snapshot.json (drizzle-kit 0.x), or <out>/<name>/migration.sql and its snapshot.json
/// (drizzle-kit 1.x). Not with `--all` (no change to compare).
pub fn drizzle_migration_present(run: &Run<'_>, _args: &[String]) -> Report {
    if run.all() {
        return Report::default();
    }
    let Some(cfg) = drizzle_config(run) else {
        return Report::default();
    };
    if cfg.schema.is_empty() {
        return run.only_note(
            NoteKind::NotEnforced,
            format!(
                "no schema path found in {}, so Drizzle schema changes are not checked for a migration",
                cfg.file
            ),
        );
    }
    let schema: Vec<String> = run
        .changed()
        .iter()
        .filter(|f| cfg.is_schema(f))
        .filter(|f| drizzle_tables(run.read(f)) != drizzle_tables(run.base(f)))
        .cloned()
        .collect();
    if schema.is_empty() {
        return Report::default();
    }
    let (mut sql, mut journal, mut snap) = (false, false, false);
    for f in run.existing() {
        if f == format!("{}/meta/_journal.json", cfg.out) {
            journal = true;
        } else if cfg.under(&f, "meta/*_snapshot.json") || cfg.under(&f, "*/snapshot.json") {
            snap = true;
        } else if cfg.under(&f, "*/migration.sql") {
            sql = true;
            // drizzle-kit 1.x keeps no journal: a <name>/migration.sql stands in for it
            journal = true;
        } else if cfg.under(&f, "*.sql") {
            sql = true;
        }
    }
    if sql && journal && snap {
        return Report::default();
    }
    // which layout the project has: drizzle-kit 0.x keeps a journal, 1.x folders
    let tracked = run.ls_files(&["-co", "--exclude-standard", "--", &cfg.out]);
    let v0 = tracked
        .iter()
        .any(|f| *f == format!("{}/meta/_journal.json", cfg.out));
    let v1 = tracked.iter().any(|f| cfg.under(f, "*/migration.sql"));
    let (what, hint) = if v0 {
        (
            "no new migration with its journal entry and snapshot",
            "Generate it (drizzle-kit generate) and keep the SQL, meta/_journal.json and the snapshot together; never write them by hand.",
        )
    } else if v1 {
        (
            "no new migration folder with its migration.sql and snapshot.json",
            "Generate it (drizzle-kit generate) and keep migration.sql and snapshot.json together; never write them by hand.",
        )
    } else {
        (
            "no new generated migration",
            "Generate it (drizzle-kit generate) and commit everything it writes; never write migrations by hand.",
        )
    };
    let mut r = Report::default();
    for f in schema {
        r.findings.push(run.at(
            &f,
            None,
            format!("the Drizzle schema changed, but {}/ has {what}", cfg.out),
        ));
    }
    r.findings.push(run.finding(hint));
    r
}

/// `forge_drizzle_applied`: migrations (SQL and snapshots in the out folder) that exist in the
/// base are not modified or deleted.
pub fn drizzle_applied(run: &Run<'_>, _args: &[String]) -> Report {
    let mut r = Report::default();
    if run.all() {
        return r;
    }
    let Some(cfg) = drizzle_config(run) else {
        return r;
    };
    for f in run.inbase() {
        if cfg.under(&f, "*.sql")
            || cfg.under(&f, "meta/*_snapshot.json")
            || cfg.under(&f, "*/snapshot.json")
        {
            let what = if run.exists(&f) {
                "modified"
            } else {
                "deleted"
            };
            r.findings.push(run.at(
                &f,
                None,
                format!(
                    "an existing migration was {what}; generate a new migration instead (drizzle-kit generate)"
                ),
            ));
        }
    }
    r
}

/// `forge_drizzle_squawk [files]`: Squawk on the changed SQL of the out folder when the dialect
/// is postgresql; with no files (`--since`, `--all`), on the newest migration.
pub fn drizzle_squawk(run: &Run<'_>, args: &[String]) -> Report {
    let Some(cfg) = drizzle_config(run) else {
        return Report::default();
    };
    if cfg.dialect != "postgresql" {
        return Report::default();
    }
    let files: Vec<String> = if args.is_empty() {
        let mut all = run.ls_files(&["--", &format!("{}/*.sql", cfg.out)]);
        all.sort();
        all.pop().into_iter().collect()
    } else {
        args.iter()
            .filter(|f| cfg.under(f, "*.sql"))
            .cloned()
            .collect()
    };
    squawk_on(run, &files)
}

/// `forge_applied_migrations <@role>...`: migrations (the files of the roles) that exist in the
/// base are not modified or deleted. Not with `--all` (no base).
pub fn applied_migrations(run: &Run<'_>, args: &[String]) -> Report {
    let mut r = Report::default();
    if run.all() {
        return r;
    }
    for f in run.inbase() {
        if args.iter().any(|role| run.role(role, &f)) {
            let what = if run.exists(&f) {
                "modified"
            } else {
                "deleted"
            };
            r.findings.push(run.at(
                &f,
                None,
                format!(
                    "an existing migration was {what}; it may already be applied somewhere, so write a new migration instead"
                ),
            ));
        }
    }
    r
}

// ---- TypeORM

/// `forge_typeorm_pending <@datasource-role> <@esm-role>`: TypeORM finds no schema change the
/// migrations lack: the migrations run on the local database, then `migration:generate
/// --check` compares the entities with it.
pub fn typeorm_pending(run: &Run<'_>, args: &[String]) -> Report {
    let ds_role = args
        .first()
        .map(String::as_str)
        .unwrap_or("@typeorm-datasource");
    let esm_role = args.get(1).map(String::as_str).unwrap_or("@esm-modules");
    let Some(ds) = run.role_files(ds_role).into_iter().next() else {
        return run.only_note(
            NoteKind::Setup,
            "not checked until the TypeORM data source is set: in forge.yaml, files: {typeorm-datasource: {set: [src/data-source.ts]}}",
        );
    };
    let bin = if [".ts", ".mts", ".cts"].iter().any(|e| ds.ends_with(e)) {
        if run.role(esm_role, &ds) {
            "typeorm-ts-node-esm"
        } else {
            "typeorm-ts-node-commonjs"
        }
    } else {
        "typeorm"
    };
    let tools = &run.opts.project_tools;
    let ran = |op: &str, a: Vec<String>| match tools.run(run.root(), bin, op, &a) {
        ToolRun::Ran(o) => Ok(o),
        ToolRun::Missing => Err(run.only_note(
            NoteKind::MissingTool,
            format!("{bin} is not installed, so entity changes are not checked for a migration"),
        )),
        ToolRun::Unsupported(why) => Err(run.only_note(NoteKind::NotEnforced, why)),
    };
    let out = match ran("migrate", vec![ds.clone()]) {
        Ok(o) => o,
        Err(r) => return r,
    };
    if out.status != 0 {
        return run.only_note(
            NoteKind::Environment,
            format!(
                "not checked: {bin} migration:run -d {ds} failed ({})",
                last_line(&out.text())
            ),
        );
    }
    let Ok(dir) = tempfile::tempdir() else {
        return Report::default();
    };
    let check = dir
        .path()
        .join("typeorm-check")
        .to_string_lossy()
        .into_owned();
    let out = match ran("migrations-check", vec![ds.clone(), check]) {
        Ok(o) => o,
        Err(r) => return r,
    };
    let text = out.text();
    if text.contains("Unexpected changes in database schema were found") {
        let mut msg = format!(
            "The entities changed, but no migration has the change: generate one ({bin} migration:generate -d {ds} <path>), review it and commit it."
        );
        for l in text.lines().filter(|l| !l.trim().is_empty()) {
            msg.push_str("\n  ");
            msg.push_str(l);
        }
        return Report {
            findings: vec![run.finding(msg)],
            notes: vec![],
        };
    }
    if text.contains("No changes in database schema were found") {
        return Report::default();
    }
    run.only_note(
        NoteKind::ForgeFailure,
        format!(
            "not checked: {bin} migration:generate --check failed ({})",
            last_line(&text)
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_table_columns() {
        let c = columns(
            "CREATE TABLE \"User\" (\"id\" INTEGER NOT NULL, \"legacyId\" TEXT, CONSTRAINT \"pk\" PRIMARY KEY (\"id\"));",
        );
        assert_eq!(
            c,
            vec![
                ("ID".to_owned(), "id".to_owned()),
                ("LEGACYID".to_owned(), "legacyId".to_owned())
            ]
        );
        assert_eq!(
            first_list("None, \"u\", [\"team_id\"], [\"id\"]"),
            "[\"team_id\"]"
        );
    }
}
