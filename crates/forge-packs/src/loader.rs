//! Loading a pack tree and rule files (`loadPacks`, `loadRuleFile`, `loadLocalRules` in
//! `src/packs.ts`): parse, validate against the schema, check the layout, type.

use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

use crate::error::{ErrorKind, LoadError};
use crate::lint::{self, Finding};
use crate::model::{Origin, Pack, PackSource, Rule};
use crate::schema::{self, Kind};
use crate::tree::{Tree, join};
use crate::validate;
use crate::yaml::{self, Parsed};

/// Directory names inside a pack that are not child packs.
pub const RESERVED: &[&str] = &["rules", "init"];

/// What loading one tree gives: packs in load order (each parent before its children,
/// siblings by name), standalone rules, and the lints' findings.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Loaded {
    pub packs: IndexMap<String, Pack>,
    pub rules: Vec<Rule>,
    pub findings: Vec<Finding>,
}

/// Where a tree's files are reported: `prefix` + their path in the tree.
pub struct Place<'a> {
    pub tree: &'a dyn Tree,
    /// Prefix of `dir` and `file` in the loaded packs and rules ("" for the built-in packs,
    /// ".forge/packs/" for local packs).
    pub prefix: &'a str,
    /// Prefix of paths in error messages and findings ("packs/" for the built-in packs).
    pub shown: &'a str,
    pub source: PackSource,
}

impl Place<'_> {
    fn path(&self, rel: &str) -> String {
        format!("{}{rel}", self.prefix)
    }
    fn shown(&self, rel: &str) -> String {
        format!("{}{rel}", self.shown)
    }
    fn err(&self, rel: &str, kind: ErrorKind, message: impl Into<String>) -> LoadError {
        LoadError {
            file: self.shown(rel),
            kind,
            message: message.into(),
        }
    }
    fn text(&self, rel: &str) -> Result<Option<String>, LoadError> {
        match self
            .tree
            .read(rel)
            .map_err(|e| self.err(rel, ErrorKind::Io, e))?
        {
            None => Ok(None),
            Some(bytes) => String::from_utf8(bytes.into_owned())
                .map(Some)
                .map_err(|_| self.err(rel, ErrorKind::Io, "not UTF-8")),
        }
    }
}

/// Parses and validates a YAML file against a schema, then types it.
fn parse_file<T: DeserializeOwned>(
    place: &Place,
    rel: &str,
    text: &str,
    kind: Kind,
) -> Result<(T, Parsed), LoadError> {
    let parsed =
        yaml::parse_strict(text).map_err(|e| place.err(rel, ErrorKind::Yaml, e.to_string()))?;
    schema::validate(kind, &parsed.value).map_err(|e| place.err(rel, ErrorKind::Schema, e))?;
    let typed = serde_json::from_value(parsed.value.clone()).map_err(|e| {
        place.err(
            rel,
            ErrorKind::Schema,
            format!(
                "invalid {}: {e} (the schema allows what the loader does not know)",
                kind.as_str()
            ),
        )
    })?;
    Ok((typed, parsed))
}

/// `loadRuleFile`: a rule file, validated, with its target-kind checks.
pub fn load_rule_file(
    place: &Place,
    rel: &str,
    origin: Origin,
    findings: &mut Vec<Finding>,
) -> Result<Rule, LoadError> {
    let text = place
        .text(rel)?
        .ok_or_else(|| place.err(rel, ErrorKind::Io, "no such file"))?;
    let (mut rule, parsed): (Rule, Parsed) = parse_file(place, rel, &text, Kind::Rule)?;
    validate::validate_rule(&rule).map_err(|e| place.err(rel, ErrorKind::Rule, e))?;
    rule.file = place.path(rel);
    rule.hash = hex(&Sha256::digest(text.as_bytes()));
    rule.origin = origin;
    rule.pack = rule
        .id
        .rsplit_once('/')
        .map(|(p, _)| p.to_string())
        .unwrap_or_default();
    // findings name the file as errors do
    findings.extend(lint::lint_rule(
        &rule,
        &place.shown(rel),
        &text,
        &parsed.scalars,
    ));
    Ok(rule)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `loadPacks`: every pack below the tree's root. A directory with a pack.yaml is a pack
/// whose id is its path; its sub-directories with a pack.yaml are its children.
/// `requires` is checked by the caller, once every tree is loaded.
pub fn load_packs(place: &Place) -> Result<Loaded, LoadError> {
    let mut out = Loaded::default();
    for (name, is_dir) in place.tree.list("").unwrap_or_default() {
        if is_dir {
            walk(place, &name, &name, None, &mut out)?;
        }
    }
    Ok(out)
}

fn walk(
    place: &Place,
    dir: &str,
    id: &str,
    parent: Option<&str>,
    out: &mut Loaded,
) -> Result<(), LoadError> {
    let pack_rel = join(dir, "pack.yaml");
    let text = place
        .text(&pack_rel)?
        .ok_or_else(|| place.err(dir, ErrorKind::Layout, "not a pack (no pack.yaml)"))?;
    let (mut pack, parsed): (Pack, Parsed) = parse_file(place, &pack_rel, &text, Kind::Pack)?;
    if pack.id != id {
        return Err(place.err(
            &pack_rel,
            ErrorKind::Layout,
            format!(
                "id \"{}\" must equal its path in the pack tree, \"{id}\"",
                pack.id
            ),
        ));
    }
    if let Some(parent) = parent.and_then(|p| out.packs.get(p))
        && pack.level != parent.level
    {
        return Err(place.err(
            &pack_rel,
            ErrorKind::Layout,
            format!(
                "level {} differs from parent {} ({})",
                pack.level, parent.id, parent.level
            ),
        ));
    }
    for c in pack
        .detect
        .iter()
        .flat_map(|d| d.content_any.iter().flatten())
    {
        if let Err(e) = fancy_regex::Regex::new(&format!("(?m){}", c.pattern)) {
            return Err(place.err(
                &pack_rel,
                ErrorKind::Schema,
                format!(
                    "detect.content_any pattern {} is not a regular expression: {e}",
                    serde_json::Value::from(c.pattern.as_str())
                ),
            ));
        }
    }
    let rules_dir = join(dir, "rules");
    let mut rules = Vec::new();
    for (f, is_dir) in place.tree.list(&rules_dir).unwrap_or_default() {
        let Some(name) = f.strip_suffix(".yaml") else {
            continue;
        };
        if is_dir {
            continue;
        }
        let rel = join(&rules_dir, &f);
        let rule = load_rule_file(place, &rel, Origin::Pack, &mut out.findings)?;
        if rule.id != format!("{id}/{name}") {
            return Err(place.err(
                &rel,
                ErrorKind::Layout,
                format!("id must be \"{id}/{name}\""),
            ));
        }
        rules.push(rule);
    }
    let context_rel = join(dir, "context.md");
    if place
        .tree
        .read(&context_rel)
        .map_err(|e| place.err(&context_rel, ErrorKind::Io, e))?
        .is_some()
    {
        return Err(place.err(
            &context_rel,
            ErrorKind::Layout,
            "a pack's context is its pack.yaml's `context:` list (items `{id?, text, when?}`), not a context.md file",
        ));
    }
    validate::validate_pack(&pack).map_err(|e| place.err(&pack_rel, ErrorKind::Schema, e))?;
    let init_rel = join(dir, "init");
    pack.dir = place.path(dir);
    pack.parent = parent.map(str::to_string);
    pack.children = Vec::new();
    pack.init_dir = place.tree.is_dir(&init_rel).then(|| place.path(&init_rel));
    pack.rules = rules;
    pack.source = place.source;
    out.findings.extend(lint::lint_pack(
        &pack,
        &place.shown(&pack_rel),
        &text,
        &parsed.scalars,
    ));
    out.packs.insert(id.to_string(), pack);
    if let Some(parent) = parent.and_then(|p| out.packs.get_mut(p)) {
        parent.children.push(id.to_string());
    }
    for (name, is_dir) in place.tree.list(dir).unwrap_or_default() {
        if RESERVED.contains(&name.as_str()) || !is_dir {
            continue;
        }
        let sub = join(dir, &name);
        if place
            .tree
            .read(&join(&sub, "pack.yaml"))
            .map_err(|e| place.err(&sub, ErrorKind::Io, e))?
            .is_some()
        {
            walk(place, &sub, &format!("{id}/{name}"), Some(id), out)?;
        } else if !place.tree.list(&sub).unwrap_or_default().is_empty() {
            return Err(place.err(&sub, ErrorKind::Layout, "not a pack (no pack.yaml)"));
        }
    }
    Ok(())
}

/// `loadLocalRules`: the `*.yaml` files of a directory, as local rules.
pub fn load_rules_dir(place: &Place) -> Result<Loaded, LoadError> {
    let mut out = Loaded::default();
    for (f, is_dir) in place.tree.list("").unwrap_or_default() {
        if is_dir || !f.ends_with(".yaml") {
            continue;
        }
        out.rules
            .push(load_rule_file(place, &f, Origin::Local, &mut out.findings)?);
    }
    Ok(out)
}
