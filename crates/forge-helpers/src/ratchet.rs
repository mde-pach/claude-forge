//! The config ratchet (`ratchet.py` and `ratchet.mjs`): each changed tool config is compared
//! with its base copy, and each loosened setting is one line, `path: what changed`.
//!
//! Python tools: ruff, ty, pytest, coverage, mypy, pyright and basedpyright (pyproject.toml,
//! ruff.toml, .ruff.toml, ty.toml, mypy.ini, .mypy.ini, pyrightconfig.json). JavaScript tools:
//! Biome (biome.json, biome.jsonc), tsconfig*.json, and package.json check scripts.

use forge_core::Report;
use forge_core::builtin::{Builtin, BuiltinCtx};
use serde_json::{Map, Value};

use crate::ACCEPT_HINT;
use crate::util::{
    Change, EMPTY, Lang, first_line, get, js_json, js_str, line, list, obj, parse_jsonc,
    parse_toml, py_eq, py_in, py_number, py_str, regex_static, strip_script,
};

pub const PYTHON_FILES: &[&str] = &[
    "pyproject.toml",
    "ruff.toml",
    ".ruff.toml",
    "ty.toml",
    "mypy.ini",
    ".mypy.ini",
    "pyrightconfig.json",
];

/// The loosened settings of one changed file, given its text now and in the base; `lang`
/// restricts it to one set of tools (None: both). A file that is not a known config, or whose
/// base copy does not parse, gives nothing.
pub fn loosened(path: &str, before: &str, after: &str, lang: Option<Lang>) -> Vec<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let mut out = Out {
        path,
        lines: Vec::new(),
    };
    if lang != Some(Lang::Js) && PYTHON_FILES.contains(&name) {
        python(&mut out, name, before, after);
    } else if lang != Some(Lang::Python) {
        js(&mut out, name, before, after);
    }
    out.lines
}

struct Out<'a> {
    path: &'a str,
    lines: Vec<String>,
}

impl Out<'_> {
    fn out(&mut self, what: impl AsRef<str>) {
        self.lines.push(format!("{}: {}", self.path, what.as_ref()));
    }
}

/// `forge_ratchet`: the loosened settings of the changed configs, each a finding, then the
/// accept hint; changes the user accepted are left out.
pub struct Ratchet;

impl Builtin for Ratchet {
    fn name(&self) -> &'static str {
        "ratchet"
    }

    fn run(&self, ctx: &BuiltinCtx, args: &[String]) -> Report {
        let (lang, _) = strip_script(args);
        let ch = Change::new(ctx);
        let mut report = Report::default();
        for p in ch.changed {
            let (Some(after), Some(before)) = (ch.now(p), ch.base_copy(p)) else {
                continue;
            };
            for l in loosened(p, &before, &after, lang) {
                if !crate::accepts(&ctx.accepted, &l) {
                    report.findings.push(line(&ctx.rule, l));
                }
            }
        }
        if !report.findings.is_empty() {
            report.findings.push(line(&ctx.rule, ACCEPT_HINT));
        }
        report
    }
}

// ---------------------------------------------------------------- Python

enum Loaded {
    Value(Value),
    Mypy(Mypy),
}

fn py_load(name: &str, text: &str) -> Result<Loaded, String> {
    if name.ends_with(".json") {
        parse_jsonc(text).map(Loaded::Value)
    } else if name.ends_with(".ini") {
        mypy_ini(text).map(Loaded::Mypy)
    } else {
        parse_toml(text).map(Loaded::Value)
    }
}

fn python(o: &mut Out, name: &str, before: &str, after: &str) {
    let Ok(b) = py_load(name, before) else { return };
    let a = match py_load(name, after) {
        Ok(a) => a,
        Err(e) => {
            o.out(format!("cannot be parsed ({})", first_line(&e)));
            return;
        }
    };
    match (b, a) {
        (Loaded::Mypy(b), Loaded::Mypy(a)) => mypy(o, &b, &a, ""),
        (Loaded::Value(b), Loaded::Value(a)) => {
            let (b, a) = (obj(Some(&b)), obj(Some(&a)));
            match name {
                "pyproject.toml" => {
                    let (tb, ta) = (obj(get(b, "tool")), obj(get(a, "tool")));
                    let t = |m: &'_ Map<String, Value>, k: &str| -> Map<String, Value> {
                        obj(get(m, k)).clone()
                    };
                    ruff(o, &t(tb, "ruff"), &t(ta, "ruff"), "tool.ruff.");
                    ty(o, &t(tb, "ty"), &t(ta, "ty"), "tool.ty.");
                    pytest(
                        o,
                        obj(get(obj(get(tb, "pytest")), "ini_options")),
                        obj(get(obj(get(ta, "pytest")), "ini_options")),
                        "tool.pytest.ini_options.",
                    );
                    coverage(o, &t(tb, "coverage"), &t(ta, "coverage"), "tool.coverage.");
                    mypy(
                        o,
                        &mypy_toml(obj(get(tb, "mypy"))),
                        &mypy_toml(obj(get(ta, "mypy"))),
                        "tool.mypy.",
                    );
                    pyright(o, &t(tb, "pyright"), &t(ta, "pyright"), "tool.pyright.");
                    pyright(
                        o,
                        &t(tb, "basedpyright"),
                        &t(ta, "basedpyright"),
                        "tool.basedpyright.",
                    );
                }
                "pyrightconfig.json" => pyright(o, b, a, ""),
                "ty.toml" => ty(o, b, a, ""),
                _ => ruff(o, b, a, ""),
            }
        }
        _ => {}
    }
}

/// `show()` of ratchet.py.
fn show(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "unset".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Array(a)) => format!(
            "[{}]",
            a.iter()
                .map(|x| show(Some(x)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Some(x) => py_str(x).replace('"', "").replace('\\', "/"),
    }
}

fn s(v: &Value) -> String {
    py_str(v)
}

fn covered(code: &Value, selectors: &[&Value]) -> bool {
    let code = s(code);
    selectors.iter().any(|sel| {
        let sel = s(sel);
        sel == "ALL" || code.starts_with(&sel)
    })
}

fn ruff(o: &mut Out, before: &Map<String, Value>, after: &Map<String, Value>, prefix: &str) {
    let lint = |cfg: &Map<String, Value>| -> Map<String, Value> {
        let mut m: Map<String, Value> = cfg
            .iter()
            .filter(|(k, _)| *k != "lint")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (k, v) in obj(get(cfg, "lint")) {
            m.insert(k.clone(), v.clone());
        }
        m
    };
    let (b, a) = (lint(before), lint(after));
    for key in ["select", "extend-select"] {
        let mut new = list(a.get("select"));
        new.extend(list(a.get("extend-select")));
        for code in list(b.get(key)) {
            if !covered(code, &new) {
                o.out(format!("{prefix}lint.{key} no longer selects {}", s(code)));
            }
        }
    }
    for key in ["ignore", "extend-ignore"] {
        for code in list(a.get(key)) {
            if !py_in(code, &list(b.get(key))) {
                o.out(format!("{prefix}lint.{key} adds {}", s(code)));
            }
        }
    }
    for key in ["per-file-ignores", "extend-per-file-ignores"] {
        let (bb, aa) = (obj(get(&b, key)), obj(get(&a, key)));
        for (glob, codes) in aa {
            for code in list(Some(codes)) {
                if !py_in(code, &list(bb.get(glob))) {
                    o.out(format!("{prefix}lint.{key} adds {} for {glob}", s(code)));
                }
            }
        }
    }
    for key in ["exclude", "extend-exclude"] {
        for x in list(after.get(key)) {
            if !py_in(x, &list(before.get(key))) {
                o.out(format!("{prefix}{key} adds {}", s(x)));
            }
        }
        for x in list(obj(get(after, "lint")).get(key)) {
            if !py_in(x, &list(obj(get(before, "lint")).get(key))) {
                o.out(format!("{prefix}lint.{key} adds {}", s(x)));
            }
        }
    }
}

fn ty_level(v: &Value) -> Option<u8> {
    match v.as_str()? {
        "ignore" => Some(0),
        "warn" => Some(1),
        "error" => Some(2),
        _ => None,
    }
}

fn ty(o: &mut Out, b: &Map<String, Value>, a: &Map<String, Value>, prefix: &str) {
    let (rb, ra) = (obj(get(b, "rules")), obj(get(a, "rules")));
    for (key, vb) in rb {
        let Some(lb) = ty_level(vb) else { continue };
        match get(ra, key) {
            None if lb > 0 => o.out(format!("{prefix}rules.{key} {} -> unset", show(Some(vb)))),
            Some(va) if ty_level(va).is_some_and(|la| la < lb) => o.out(format!(
                "{prefix}rules.{key} {} -> {}",
                show(Some(vb)),
                show(Some(va))
            )),
            _ => {}
        }
    }
    let (tb, ta) = (obj(get(b, "terminal")), obj(get(a, "terminal")));
    if tb.get("error-on-warning") == Some(&Value::Bool(true))
        && ta.get("error-on-warning") != Some(&Value::Bool(true))
    {
        o.out(format!(
            "{prefix}terminal.error-on-warning true -> {}",
            show(ta.get("error-on-warning"))
        ));
    }
    let (sb, sa) = (obj(get(b, "src")), obj(get(a, "src")));
    for x in list(sa.get("exclude")) {
        if !py_in(x, &list(sb.get("exclude"))) {
            o.out(format!("{prefix}src.exclude adds {}", s(x)));
        }
    }
    if list(a.get("overrides")).len() > list(b.get("overrides")).len() {
        o.out(format!("{prefix}overrides adds an entry"));
    }
}

fn pytest(o: &mut Out, b: &Map<String, Value>, a: &Map<String, Value>, prefix: &str) {
    let opts = |m: &Map<String, Value>| -> Vec<Value> {
        match m.get("addopts") {
            Some(Value::Array(x)) => x.clone(),
            None => vec![],
            Some(v) => s(v)
                .split_whitespace()
                .map(|t| Value::String(t.to_owned()))
                .collect(),
        }
    };
    let (tb, ta) = (opts(b), opts(a));
    let loose = regex_static(
        r"^(--ignore(-glob)?|--deselect|-k|-m|--continue-on-collection-errors|--lf|--last-failed|--sw|--stepwise)(=|$)",
    );
    for t in &ta {
        if let Some(ts) = t.as_str()
            && loose.is_match(ts)
            && !tb.iter().any(|x| py_eq(x, t))
        {
            o.out(format!("{prefix}addopts adds {ts}"));
        }
    }
    for x in list(get(b, "testpaths")) {
        if !py_in(x, &list(get(a, "testpaths"))) {
            o.out(format!("{prefix}testpaths no longer has {}", s(x)));
        }
    }
    for x in list(get(a, "norecursedirs")) {
        if !py_in(x, &list(get(b, "norecursedirs"))) {
            o.out(format!("{prefix}norecursedirs adds {}", s(x)));
        }
    }
    if b.get("xfail_strict") == Some(&Value::Bool(true))
        && a.get("xfail_strict") != Some(&Value::Bool(true))
    {
        o.out(format!(
            "{prefix}xfail_strict true -> {}",
            show(a.get("xfail_strict"))
        ));
    }
}

fn coverage(o: &mut Out, b: &Map<String, Value>, a: &Map<String, Value>, prefix: &str) {
    let (rb, ra) = (obj(get(b, "report")), obj(get(a, "report")));
    let (fb, fa) = (rb.get("fail_under"), ra.get("fail_under"));
    if let Some(nb) = fb.and_then(py_number)
        && fa.and_then(py_number).is_none_or(|na| na < nb)
    {
        o.out(format!(
            "{prefix}report.fail_under {} -> {}",
            show(fb),
            show(fa)
        ));
    }
    for (sect, key) in [
        ("run", "omit"),
        ("report", "omit"),
        ("report", "exclude_lines"),
        ("report", "exclude_also"),
    ] {
        for x in list(obj(get(a, sect)).get(key)) {
            if !py_in(x, &list(obj(get(b, sect)).get(key))) {
                o.out(format!("{prefix}{sect}.{key} adds {}", s(x)));
            }
        }
    }
}

#[derive(Default)]
struct Mypy {
    global: Map<String, Value>,
    overrides: Vec<(String, Map<String, Value>)>,
}

impl Mypy {
    fn module(&self, m: &str) -> &Map<String, Value> {
        self.overrides
            .iter()
            .find(|(k, _)| k == m)
            .map_or(&EMPTY, |(_, v)| v)
    }

    fn update(&mut self, module: String, values: impl IntoIterator<Item = (String, Value)>) {
        let i = match self.overrides.iter().position(|(k, _)| *k == module) {
            Some(i) => i,
            None => {
                self.overrides.push((module, Map::new()));
                self.overrides.len() - 1
            }
        };
        for (k, v) in values {
            self.overrides[i].1.insert(k, v);
        }
    }
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(true)) => true,
        Some(Value::String(s)) => {
            matches!(
                s.trim().to_lowercase().as_str(),
                "true" | "yes" | "on" | "1"
            )
        }
        _ => false,
    }
}

fn mypy(o: &mut Out, b: &Mypy, a: &Mypy, prefix: &str) {
    let (gb, ga) = (&b.global, &a.global);
    for (key, vb) in gb {
        let strictness = key == "strict"
            || key.starts_with("disallow_")
            || key.starts_with("warn_")
            || key.starts_with("strict_");
        if strictness && truthy(Some(vb)) && !truthy(ga.get(key)) {
            o.out(format!("{prefix}{key} true -> {}", show(ga.get(key))));
        }
    }
    for key in ["ignore_missing_imports", "ignore_errors"] {
        if truthy(ga.get(key)) && !truthy(gb.get(key)) {
            o.out(format!("{prefix}{key} {} -> true", show(gb.get(key))));
        }
        for (module, oa) in &a.overrides {
            if truthy(oa.get(key)) && !truthy(b.module(module).get(key)) {
                o.out(format!("{prefix}overrides adds {key} for {module}"));
            }
        }
    }
}

fn mypy_toml(cfg: &Map<String, Value>) -> Mypy {
    let mut m = Mypy {
        global: cfg
            .iter()
            .filter(|(k, _)| *k != "overrides")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        ..Mypy::default()
    };
    for ov in list(cfg.get("overrides")) {
        let Value::Object(ov) = ov else { continue };
        for module in list(ov.get("module")) {
            m.update(
                s(module),
                ov.iter()
                    .filter(|(k, _)| *k != "module")
                    .map(|(k, v)| (k.clone(), v.clone())),
            );
        }
    }
    m
}

fn mypy_ini(text: &str) -> Result<Mypy, String> {
    let p = crate::util::Ini::parse(text)?;
    let values = |sect: &str| -> Map<String, Value> {
        p.items(sect)
            .into_iter()
            .map(|(k, v)| {
                let v = match v.trim().to_lowercase().as_str() {
                    "true" => Value::Bool(true),
                    "false" => Value::Bool(false),
                    _ => Value::String(v),
                };
                (k, v)
            })
            .collect()
    };
    let mut m = Mypy::default();
    for (sect, _) in &p.sections {
        if let Some(mods) = sect.strip_prefix("mypy-") {
            for module in mods.split(',') {
                m.update(module.trim().to_owned(), values(sect));
            }
        }
    }
    if p.has_section("mypy") {
        m.global = values("mypy");
    }
    Ok(m)
}

fn mode(v: Option<&Value>) -> Option<u8> {
    Some(match v?.as_str()? {
        "off" => 0,
        "basic" => 1,
        "standard" => 2,
        "recommended" => 3,
        "strict" => 4,
        "all" => 5,
        _ => return None,
    })
}

fn severity(v: Option<&Value>) -> Option<u8> {
    match v {
        Some(Value::Bool(false)) => Some(0),
        Some(Value::String(s)) => match s.as_str() {
            "none" => Some(0),
            "information" => Some(1),
            "warning" => Some(2),
            "error" => Some(3),
            _ => None,
        },
        _ => None,
    }
}

fn pyright(o: &mut Out, b: &Map<String, Value>, a: &Map<String, Value>, prefix: &str) {
    let (mb, ma) = (get(b, "typeCheckingMode"), get(a, "typeCheckingMode"));
    if let Some(lb) = mode(mb)
        && (ma.is_none() || mode(ma).is_some_and(|la| la < lb))
    {
        o.out(format!(
            "{prefix}typeCheckingMode {} -> {}",
            show(mb),
            show(ma)
        ));
    }
    for (key, vb) in b {
        if !key.starts_with("report") {
            continue;
        }
        let va = a.get(key);
        let (sb, sa) = (severity(Some(vb)), severity(va));
        let on = *vb == Value::Bool(true) || sb.is_some_and(|x| x > 0);
        if on && !a.contains_key(key) {
            o.out(format!("{prefix}{key} {} -> unset", show(Some(vb))));
        } else if (on && sa == Some(0)) || matches!((sb, sa), (Some(x), Some(y)) if y < x) {
            o.out(format!("{prefix}{key} {} -> {}", show(Some(vb)), show(va)));
        }
    }
}

// ---------------------------------------------------------------- JavaScript

fn js(o: &mut Out, name: &str, before: &str, after: &str) {
    type Kind = fn(&mut Out, &Map<String, Value>, &Map<String, Value>);
    let kind: Kind = if name == "biome.json" || name == "biome.jsonc" {
        biome
    } else if regex_static(r"^tsconfig.*\.json$").is_match(name) {
        tsconfig
    } else if name == "package.json" {
        scripts
    } else {
        return;
    };
    let Ok(b) = parse_jsonc(before) else { return };
    let a = match parse_jsonc(after) {
        Ok(a) => a,
        Err(e) => {
            o.out(format!("cannot be parsed ({})", first_line(&e)));
            return;
        }
    };
    kind(o, obj(Some(&b)), obj(Some(&a)));
}

/// `show()` of ratchet.mjs.
fn jshow(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "unset".into(),
        Some(Value::Array(a)) => format!(
            "[{}]",
            a.iter()
                .map(|x| jshow(Some(x)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Some(Value::Object(_)) => "{…}".into(),
        Some(x) => js_str(x).replace(['"', '\\'], ""),
    }
}

fn level_of(v: &str) -> Option<i32> {
    Some(match v {
        "off" => 0,
        "info" => 1,
        "warn" | "on" => 2,
        "error" => 3,
        _ => return None,
    })
}

fn rule_level(v: Option<&Value>) -> Option<&Value> {
    match v {
        Some(Value::String(_)) => v,
        _ => obj(v).get("level"),
    }
}

fn biome(o: &mut Out, b: &Map<String, Value>, a: &Map<String, Value>) {
    let (lb, la) = (obj(b.get("linter")), obj(a.get("linter")));
    let f = Value::Bool(false);
    if lb.get("enabled") != Some(&f) && la.get("enabled") == Some(&f) {
        o.out("linter.enabled true -> false");
    }
    if obj(b.get("formatter")).get("enabled") != Some(&f)
        && obj(a.get("formatter")).get("enabled") == Some(&f)
    {
        o.out("formatter.enabled true -> false");
    }
    let (rb, ra) = (obj(lb.get("rules")), obj(la.get("rules")));
    if rb.get("recommended") != Some(&f) && ra.get("recommended") == Some(&f) {
        let was = get(rb, "recommended").cloned().unwrap_or(Value::Bool(true));
        o.out(format!(
            "linter.rules.recommended {} -> false",
            jshow(Some(&was))
        ));
    }
    // `preset` (Biome 2.6, replacing `recommended`): none < recommended < all
    let rank = |v: Option<&Value>| match v.and_then(Value::as_str) {
        Some("none") => 0,
        Some("all") => 2,
        _ => 1,
    };
    if ra.contains_key("preset") && rank(ra.get("preset")) < rank(rb.get("preset")) {
        o.out(format!(
            "linter.rules.preset {} -> {}",
            jshow(Some(
                rb.get("preset")
                    .unwrap_or(&Value::String("recommended".into()))
            )),
            jshow(ra.get("preset"))
        ));
    }
    for (group, gb) in rb {
        if group == "recommended" || group == "preset" {
            continue;
        }
        let ga = ra.get(group);
        if let Value::String(gb) = gb {
            let now = ga.and_then(Value::as_str);
            let lgb = level_of(gb).unwrap_or(0);
            match now {
                Some(now) if lgb > level_of(now).unwrap_or(-1) => {
                    o.out(format!("linter.rules.{group} {gb} -> {now}"));
                }
                None if lgb > 0
                    && !matches!(ga, Some(Value::Object(_) | Value::Array(_) | Value::Null)) =>
                {
                    o.out(format!("linter.rules.{group} {gb} -> unset"));
                }
                _ => {}
            }
            continue;
        }
        for (name, vb) in obj(Some(gb)) {
            let Some(Value::String(before)) = rule_level(Some(vb)) else {
                continue;
            };
            let Some(lbefore) = level_of(before) else {
                continue;
            };
            let after = match ga {
                Some(Value::String(_)) => ga,
                _ => rule_level(obj(ga).get(name)),
            };
            match after {
                None => {
                    if lbefore > 0 {
                        o.out(format!("linter.rules.{group}.{name} {before} -> unset"));
                    }
                }
                Some(after) => {
                    let la = after.as_str().and_then(level_of).unwrap_or(0);
                    if la < lbefore {
                        o.out(format!(
                            "linter.rules.{group}.{name} {before} -> {}",
                            js_str(after)
                        ));
                    }
                }
            }
        }
    }
    let negated = |cfg: &Map<String, Value>| -> Vec<String> {
        list(cfg.get("includes"))
            .into_iter()
            .filter_map(|g| g.as_str().filter(|g| g.starts_with('!')).map(str::to_owned))
            .collect()
    };
    for (place, cb, ca) in [
        ("files", obj(b.get("files")), obj(a.get("files"))),
        ("linter", lb, la),
    ] {
        let nb = negated(cb);
        for g in negated(ca) {
            if !nb.contains(&g) {
                o.out(format!("{place}.includes adds {g}"));
            }
        }
        let ib = list(cb.get("ignore"));
        for g in list(ca.get("ignore")) {
            if !ib.contains(&g) {
                o.out(format!("{place}.ignore adds {}", js_str(g)));
            }
        }
    }
    let ov = |m: &Map<String, Value>| js_json(m.get("overrides").unwrap_or(&Value::Array(vec![])));
    if ov(b) != ov(a) {
        let before: Vec<String> = list(b.get("overrides")).into_iter().map(js_json).collect();
        for o_ in list(a.get("overrides")) {
            if !before.contains(&js_json(o_)) {
                let m = obj(Some(o_));
                let globs = match m.get("includes").filter(|v| !v.is_null()) {
                    Some(v) => list(Some(v)),
                    None => list(m.get("include")),
                };
                let joined = globs
                    .iter()
                    .map(|g| {
                        if g.is_null() {
                            String::new()
                        } else {
                            js_str(g)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                o.out(format!(
                    "overrides adds or changes an entry for {}",
                    jshow(Some(&Value::String(joined)))
                ));
            }
        }
    }
}

const STRICT: &[&str] = &[
    "strict",
    "noImplicitAny",
    "strictNullChecks",
    "strictFunctionTypes",
    "strictBindCallApply",
    "strictPropertyInitialization",
    "strictBuiltinIteratorReturn",
    "noImplicitThis",
    "useUnknownInCatchVariables",
    "alwaysStrict",
    "noUncheckedIndexedAccess",
    "exactOptionalPropertyTypes",
    "noImplicitReturns",
    "noFallthroughCasesInSwitch",
    "noImplicitOverride",
    "noPropertyAccessFromIndexSignature",
    "noUnusedLocals",
    "noUnusedParameters",
    "checkJs",
];

fn tsconfig(o: &mut Out, b: &Map<String, Value>, a: &Map<String, Value>) {
    let (cb, ca) = (obj(b.get("compilerOptions")), obj(a.get("compilerOptions")));
    let t = Value::Bool(true);
    for k in STRICT {
        if cb.get(*k) == Some(&t) && ca.get(*k) != Some(&t) {
            o.out(format!("compilerOptions.{k} true -> {}", jshow(ca.get(*k))));
        }
    }
    if cb.get("skipLibCheck") != Some(&t) && ca.get("skipLibCheck") == Some(&t) {
        o.out(format!(
            "compilerOptions.skipLibCheck {} -> true",
            jshow(cb.get("skipLibCheck"))
        ));
    }
    if !cb.contains_key("ignoreDeprecations") && ca.contains_key("ignoreDeprecations") {
        o.out(format!(
            "compilerOptions.ignoreDeprecations unset -> {}",
            jshow(ca.get("ignoreDeprecations"))
        ));
    }
    let eb = list(b.get("exclude"));
    for x in list(a.get("exclude")) {
        if !eb.contains(&x) {
            o.out(format!("exclude adds {}", jshow(Some(x))));
        }
    }
    let ia = list(a.get("include"));
    for x in list(b.get("include")) {
        if !ia.contains(&x) {
            o.out(format!("include no longer has {}", jshow(Some(x))));
        }
    }
    let ext = |m: &Map<String, Value>| m.get("extends").map(js_json);
    if ext(b) != ext(a) {
        o.out(format!(
            "extends {} -> {}",
            jshow(b.get("extends")),
            jshow(a.get("extends"))
        ));
    }
}

const LOOSE: &[&str] = &[
    r"--passWithNoTests\b",
    r"--pass-with-no-tests\b",
    r"\|\|\s*(true|exit 0|:)\b",
    r"--no-errors-on-unmatched\b",
    r"--testPathIgnorePatterns\b",
    r"--exclude\b",
    r"--ignore\b",
    r"--grep-invert\b",
    r"--skip\b",
    r"--diagnostic-level[= ](info|warn)\b",
    r"--max-warnings[= ]\d+",
    r"--bail[= ]0\b",
    r"--silent\b",
];

fn scripts(o: &mut Out, b: &Map<String, Value>, a: &Map<String, Value>) {
    let (sb, sa) = (obj(b.get("scripts")), obj(a.get("scripts")));
    let check = regex_static(r"(?i)test|check|lint|type|e2e|ci|verify");
    static LOOSE_RE: std::sync::LazyLock<Vec<regex::Regex>> = std::sync::LazyLock::new(|| {
        LOOSE
            .iter()
            .map(|p| {
                regex::Regex::new(&format!("(?-u:{p})"))
                    .unwrap_or_else(|_| regex::Regex::new(p).expect("regex"))
            })
            .collect()
    });
    for (name, cmd) in sb {
        if !check.is_match(name) {
            continue;
        }
        let Some(now) = sa.get(name) else {
            o.out(format!("scripts.{name} removed"));
            continue;
        };
        let (now, cmd) = (js_str(now), js_str(cmd));
        for re in LOOSE_RE.iter() {
            if let Some(m) = re.find(&now)
                && !re.is_match(&cmd)
            {
                o.out(format!(
                    "scripts.{name} adds {}",
                    jshow(Some(&Value::String(m.as_str().trim().to_owned())))
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruff_ignore() {
        let l = loosened(
            "pyproject.toml",
            "[tool.ruff.lint]\nselect = [\"E\", \"F\"]\n",
            "[tool.ruff.lint]\nselect = [\"E\", \"F\"]\nignore = [\"F401\"]\n",
            None,
        );
        assert_eq!(l, ["pyproject.toml: tool.ruff.lint.ignore adds F401"]);
    }

    #[test]
    fn mypy_ini_false() {
        let l = loosened(
            "mypy.ini",
            "[mypy]\ndisallow_untyped_defs = True\n",
            "[mypy]\ndisallow_untyped_defs = False\n",
            None,
        );
        assert_eq!(l, ["mypy.ini: disallow_untyped_defs true -> false"]);
    }

    #[test]
    fn biome_rule_off() {
        let l = loosened(
            "biome.json",
            r#"{"linter":{"rules":{"suspicious":{"noExplicitAny":"error"}}}}"#,
            r#"{"linter":{"rules":{"suspicious":{"noExplicitAny":"off"}}}}"#,
            None,
        );
        assert_eq!(
            l,
            ["biome.json: linter.rules.suspicious.noExplicitAny error -> off"]
        );
    }

    #[test]
    fn biome_preset_lowered() {
        let l = loosened(
            "biome.json",
            r#"{"linter":{"rules":{"preset":"recommended"}}}"#,
            r#"{"linter":{"rules":{"preset":"none"}}}"#,
            None,
        );
        assert_eq!(l, ["biome.json: linter.rules.preset recommended -> none"]);
        let raised = loosened(
            "biome.json",
            r#"{"linter":{"rules":{"recommended":true}}}"#,
            r#"{"linter":{"rules":{"preset":"all"}}}"#,
            None,
        );
        assert!(raised.is_empty(), "{raised:?}");
    }
}
