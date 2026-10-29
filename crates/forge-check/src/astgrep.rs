//! ast-grep targets, with ast-grep linked as a library: each rule's `rule`, `constraints` and
//! `utils` become an ast-grep rule config in memory (no files), compiled once per run.
//!
//! Languages are ast-grep's, as Forge's sgconfig set them (`src/render/astgrep.ts`): a
//! `python` rule is Python (`.py`, `.pyi`, …), `typescript` and `tsx` rules are Tsx (`.tsx`,
//! and `.ts`, `.mts`, `.cts` through `languageGlobs`), `yaml` is Yaml.

use std::collections::BTreeMap;
use std::path::Path;

use ast_grep_config::{CombinedScan, GlobalRules, RuleConfig, SerializableRuleConfig};
use ast_grep_core::tree_sitter::LanguageExt;
use ast_grep_language::SupportLang;
use forge_packs::model::{AstGrepTarget, Language, Mode, Rule, Target};
use serde_json::{Map, Value};

use crate::scope::{Roles, RuleScope};

/// The ast-grep language of a rule's scope language.
pub fn language(lang: Language) -> Option<SupportLang> {
    match lang {
        Language::Python => Some(SupportLang::Python),
        Language::Typescript | Language::Tsx => Some(SupportLang::Tsx),
        Language::Yaml => Some(SupportLang::Yaml),
        Language::Any => None,
    }
}

/// The ast-grep language a file is parsed as (sgconfig's `languageGlobs` included).
pub fn file_language(path: &str) -> Option<SupportLang> {
    let ext = path.rsplit_once('.').map(|(_, e)| e)?;
    match ext {
        "py" | "py3" | "pyi" | "bzl" | "bazel" => Some(SupportLang::Python),
        "tsx" | "ts" | "mts" | "cts" => Some(SupportLang::Tsx),
        "yaml" | "yml" => Some(SupportLang::Yaml),
        _ => None,
    }
}

/// One compiled ast-grep target.
pub struct Compiled {
    pub rule: Rule,
    pub target: AstGrepTarget,
    pub lang: SupportLang,
    pub config: RuleConfig<SupportLang>,
    pub scope: RuleScope,
}

impl Compiled {
    pub fn id(&self) -> &str {
        &self.rule.id
    }

    pub fn mode(&self) -> Mode {
        self.target.mode
    }

    pub fn typed(&self) -> bool {
        self.target.where_.is_some()
    }
}

/// The ast-grep rule config of a target, as JSON (ast-grep's YAML shape).
pub fn rule_config_value(rule: &Rule, target: &AstGrepTarget, lang: SupportLang) -> Value {
    let mut doc = Map::new();
    doc.insert("id".into(), Value::String(rule.id.clone()));
    doc.insert(
        "language".into(),
        Value::String(
            match lang {
                SupportLang::Python => "Python",
                SupportLang::Yaml => "Yaml",
                _ => "Tsx",
            }
            .into(),
        ),
    );
    doc.insert(
        "severity".into(),
        Value::String(
            if target.mode == Mode::Advise {
                "warning"
            } else {
                "error"
            }
            .into(),
        ),
    );
    doc.insert("message".into(), Value::String(rule.description.clone()));
    doc.insert("rule".into(), Value::Object(target.rule.clone()));
    if let Some(c) = &target.constraints {
        doc.insert("constraints".into(), Value::Object(c.clone()));
    }
    if let Some(u) = &target.utils {
        doc.insert("utils".into(), Value::Object(u.clone()));
    }
    Value::Object(doc)
}

/// Compiles a rule's ast-grep target.
pub fn compile(
    rule: &Rule,
    target: &AstGrepTarget,
    roles: &Roles,
    dirs: Option<&[String]>,
) -> Result<Compiled, String> {
    let lang = language(rule.scope.language).ok_or_else(|| {
        format!(
            "{}: ast-grep targets need a concrete scope.language",
            rule.id
        )
    })?;
    let value = rule_config_value(rule, target, lang);
    let ser: SerializableRuleConfig<SupportLang> =
        serde_json::from_value(value).map_err(|e| format!("{}: {e}", rule.id))?;
    let config = RuleConfig::try_from(ser, &GlobalRules::default())
        .map_err(|e| format!("{}: {e}", rule.id))?;
    Ok(Compiled {
        rule: rule.clone(),
        target: target.clone(),
        lang,
        config,
        scope: RuleScope::new(rule, roles, dirs, None),
    })
}

/// Compiles ast-grep rules written as ast-grep YAML (inline rules, several documents).
pub fn compile_yaml(yaml: &str) -> Result<Vec<RuleConfig<SupportLang>>, String> {
    ast_grep_config::from_yaml_string(yaml, &GlobalRules::default()).map_err(|e| e.to_string())
}

/// A rule's first ast-grep target.
pub fn target_of(rule: &Rule) -> Option<&AstGrepTarget> {
    rule.enforce.iter().find_map(|t| match t {
        Target::AstGrep(a) => Some(a),
        _ => None,
    })
}

/// One match, as the types engine and the reports read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstMatch {
    pub rule: String,
    pub path: String,
    /// 1-based.
    pub line: u32,
    pub end_line: u32,
    /// 1-based, in characters.
    pub column: u32,
    pub byte_start: usize,
    pub byte_end: usize,
    pub text: String,
    /// The matched metavariables: name (without `$`) → (text, byte offset).
    pub vars: BTreeMap<String, (String, usize)>,
}

/// The matches of the given rules in one source text (`path` gives its language: the rules of
/// another language are skipped). Suppression comments (`ast-grep-ignore`) apply, as in a scan.
pub fn scan_source(rules: &[&Compiled], path: &str, source: &str) -> Vec<AstMatch> {
    let Some(lang) = file_language(path) else {
        return Vec::new();
    };
    let own: Vec<&RuleConfig<SupportLang>> = rules
        .iter()
        .filter(|c| c.lang == lang)
        .map(|c| &c.config)
        .collect();
    if own.is_empty() {
        return Vec::new();
    }
    scan_configs(&own, lang, path, source)
}

/// The matches of ast-grep rule configs in a source text parsed as `lang`.
pub fn scan_configs(
    configs: &[&RuleConfig<SupportLang>],
    lang: SupportLang,
    path: &str,
    source: &str,
) -> Vec<AstMatch> {
    let root = lang.ast_grep(source);
    let scan = CombinedScan::new(configs.to_vec());
    let result = scan.scan(&root, false);
    let mut out = Vec::new();
    for (cfg, matches) in result.matches {
        for m in matches {
            let start = m.start_pos();
            let end = m.end_pos();
            let mut vars = BTreeMap::new();
            let env = m.get_env();
            for v in env.get_matched_variables() {
                let name = match &v {
                    ast_grep_core::meta_var::MetaVariable::Capture(n, _) => n.clone(),
                    ast_grep_core::meta_var::MetaVariable::MultiCapture(n) => n.clone(),
                    _ => continue,
                };
                if let Some(node) = env.get_match(&name) {
                    vars.insert(name, (node.text().to_string(), node.range().start));
                }
            }
            out.push(AstMatch {
                rule: cfg.id.clone(),
                path: path.to_string(),
                line: start.line() as u32 + 1,
                end_line: end.line() as u32 + 1,
                column: start.column(&m) as u32 + 1,
                byte_start: m.range().start,
                byte_end: m.range().end,
                text: m.text().to_string(),
                vars,
            });
        }
    }
    out.sort_by(|a, b| {
        (a.path.as_str(), a.byte_start, a.rule.as_str()).cmp(&(
            b.path.as_str(),
            b.byte_start,
            b.rule.as_str(),
        ))
    });
    out
}

/// The matches of `rules` in the files `paths` under `dir` (each rule on the files of its
/// scope).
pub fn scan_files(rules: &[&Compiled], dir: &Path, paths: &[String]) -> Vec<AstMatch> {
    let mut out = Vec::new();
    for p in paths {
        let Some(lang) = file_language(p) else {
            continue;
        };
        let applicable: Vec<&Compiled> = rules
            .iter()
            .copied()
            .filter(|c| c.lang == lang && c.scope.files_match(p))
            .collect();
        if applicable.is_empty() {
            continue;
        }
        let Ok(bytes) = std::fs::read(dir.join(p)) else {
            continue;
        };
        let source = String::from_utf8_lossy(&bytes);
        out.extend(scan_source(&applicable, p, &source));
    }
    out
}

/// `ast-grep test` semantics for one example: does the rule find something in it.
pub fn example_matches(c: &Compiled, code: &str) -> bool {
    let root = c.lang.ast_grep(code);
    root.root().find(&c.config.matcher).is_some()
}

/// Answers a rule's `where` conditions (§15.1) on its matches: the types engine.
pub trait TypeOracle: Send + Sync {
    /// For each match, whether the conditions hold (a match it cannot confirm does not stand),
    /// and the notes for the person: what it could not check, and why.
    fn confirm(
        &self,
        root: &Path,
        rule: &Compiled,
        matches: &[AstMatch],
    ) -> (Vec<bool>, Vec<forge_core::Note>);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(yaml_rule: Value, lang: Language) -> Rule {
        serde_json::from_value(serde_json::json!({
            "id": "t/r", "description": "Do not.", "scope": {"language": lang},
            "enforce": [{"target": "ast-grep", "mode": "forbid", "rule": yaml_rule}],
        }))
        .unwrap()
    }

    #[test]
    fn compiles_and_matches() {
        let r = rule(serde_json::json!({"pattern": "eval($X)"}), Language::Python);
        let c = compile(&r, target_of(&r).unwrap(), &Roles::defaults(), None).unwrap();
        assert!(example_matches(&c, "eval(x)\n"));
        assert!(!example_matches(&c, "evaluate(x)\n"));
        let m = scan_source(&[&c], "a.py", "x = 1\neval(y)\n");
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].line, m[0].column), (2, 1));
        assert_eq!(m[0].vars["X"].0, "y");
        // suppressed as a scan suppresses it
        let m = scan_source(&[&c], "a.py", "eval(y)  # ast-grep-ignore\n");
        assert!(m.is_empty());
    }
}
