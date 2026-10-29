//! Foreign key columns a change adds to SQLAlchemy models get an index that leads with them
//! (`fk_index.py`).
//!
//! For each changed .py file: the foreign key columns of each declarative class
//! (`mapped_column(...)` / `Column(...)` with a `ForeignKey(...)` argument, or a
//! `ForeignKeyConstraint` in `__table_args__`) and of each `Table(...)` call that were not in the
//! base copy, and that no index leads with: `index=True` or `unique=True` on the column,
//! `primary_key=True` when it is the table's first primary key column, or an `Index` /
//! `UniqueConstraint` / `PrimaryKeyConstraint` whose first column it is.

use std::collections::HashSet;

use forge_core::Report;
use forge_core::builtin::{Builtin, BuiltinCtx};
use indexmap_lite::OrderedMap;

use crate::pyast::{self, N, args, assign, body, elements, ident, last_name, string, walk};
use crate::util::{Change, at};

pub struct FkIndex;

const MESSAGE: &str = "has no index that leads with it; PostgreSQL and SQLite do not index referencing columns, so joins on it and deletes of the referenced rows scan the table. If it is queried or the parent rows are deleted, add index=True (or an Index whose first column it is); otherwise say why not.";

impl Builtin for FkIndex {
    fn name(&self) -> &'static str {
        "fk_index"
    }

    fn run(&self, ctx: &BuiltinCtx, _args: &[String]) -> Report {
        let ch = Change::new(ctx);
        let mut r = Report::default();
        for path in ch.changed {
            if !path.ends_with(".py") || path.contains("/versions/") {
                continue;
            }
            let Some(tree) = ch.now(path).and_then(|t| pyast::parse(&t)) else {
                continue;
            };
            let base = ch
                .base_copy(path)
                .and_then(|t| pyast::parse(&t))
                .map(|bt| tables(&bt.root()))
                .unwrap_or_default();
            for (key, t) in tables(&tree.root()).entries {
                let old = base.get(&key);
                let mut cols = t.unindexed();
                cols.sort_by_key(|c| c.1);
                for (col, line) in cols {
                    if old.is_some_and(|o| o.fks.contains_key(&col)) {
                        continue;
                    }
                    r.findings.push(at(
                        &ctx.rule,
                        path,
                        line,
                        format!("foreign key column {col} {MESSAGE}"),
                    ));
                }
            }
        }
        r
    }
}

/// Insertion-ordered maps, enough for the few keys a model has.
mod indexmap_lite {
    #[derive(Debug, Clone)]
    pub struct OrderedMap<V> {
        pub entries: Vec<(String, V)>,
    }

    impl<V> Default for OrderedMap<V> {
        fn default() -> Self {
            OrderedMap { entries: vec![] }
        }
    }

    impl<V> OrderedMap<V> {
        pub fn get(&self, k: &str) -> Option<&V> {
            self.entries.iter().find(|(x, _)| x == k).map(|(_, v)| v)
        }

        pub fn contains_key(&self, k: &str) -> bool {
            self.get(k).is_some()
        }

        /// Inserts when absent (`setdefault`).
        pub fn insert_new(&mut self, k: String, v: V) {
            if !self.contains_key(&k) {
                self.entries.push((k, v));
            }
        }

        /// Inserts or replaces (`d[k] = v`).
        pub fn set(&mut self, k: String, v: V) {
            match self.entries.iter_mut().find(|(x, _)| *x == k) {
                Some(e) => e.1 = v,
                None => self.entries.push((k, v)),
            }
        }
    }
}

#[derive(Debug, Default, Clone)]
struct Table {
    fks: OrderedMap<usize>,
    leading: HashSet<String>,
    pks: Vec<String>,
}

fn kw_true(call: &N, name: &str) -> bool {
    args(call).kw.iter().any(|(k, v)| k == name && v.kind() == "true")
}

fn func_last(call: &N) -> String {
    call.field("function").map(|f| last_name(&f)).unwrap_or_default()
}

fn has_fk(call: &N) -> bool {
    args(call)
        .pos
        .iter()
        .any(|a| a.kind() == "call" && func_last(a) == "ForeignKey")
}

/// A column named by a string, a name or an attribute (`Post.author_id`, `author_id`).
fn col_ref(n: &N) -> Option<String> {
    if let Some(s) = string(n) {
        return Some(s.rsplit('.').next().unwrap_or_default().to_owned());
    }
    matches!(&*n.kind(), "identifier" | "attribute").then(|| last_name(n))
}

impl Table {
    fn column(&mut self, name: String, call: &N) {
        if has_fk(call) {
            self.fks.insert_new(name.clone(), pyast::line(call));
        }
        if kw_true(call, "index") || kw_true(call, "unique") {
            self.leading.insert(name.clone());
        }
        if kw_true(call, "primary_key") {
            self.pks.push(name);
        }
    }

    fn constraint(&mut self, call: &N) {
        let kind = func_last(call);
        let a = args(call).pos;
        if kind == "ForeignKeyConstraint" {
            if let Some(first) = a.first().and_then(elements).and_then(|e| e.into_iter().next())
                && let Some(name) = col_ref(&first)
            {
                self.fks.insert_new(name, pyast::line(call));
            }
            return;
        }
        let rest = if kind == "Index" { a.get(1..).unwrap_or_default() } else { &a[..] };
        if matches!(&*kind, "Index" | "UniqueConstraint" | "PrimaryKeyConstraint")
            && let Some(name) = rest.first().and_then(col_ref)
        {
            self.leading.insert(name);
        }
    }

    fn unindexed(&self) -> Vec<(String, usize)> {
        let mut lead = self.leading.clone();
        if let Some(p) = self.pks.first() {
            lead.insert(p.clone());
        }
        self.fks
            .entries
            .iter()
            .filter(|(c, _)| !lead.contains(c))
            .cloned()
            .collect()
    }
}

/// The tables of a module that have foreign keys: `class <name>` and `table <name>`.
fn tables(root: &N) -> OrderedMap<Table> {
    let mut out = OrderedMap::default();
    for node in walk(root) {
        if node.kind() == "class_definition" {
            let mut t = Table::default();
            for st in body(&node) {
                let Some(a) = assign(&st) else { continue };
                if a.aug {
                    continue;
                }
                let Some(value) = a.value else { continue };
                for target in &a.targets {
                    let Some(id) = ident(target) else { continue };
                    if id == "__table_args__" {
                        for sub in walk(&value) {
                            if sub.kind() == "call" {
                                t.constraint(&sub);
                            }
                        }
                    } else if value.kind() == "call"
                        && matches!(&*func_last(&value), "mapped_column" | "Column")
                    {
                        let named = args(&value).pos.first().and_then(string);
                        t.column(named.unwrap_or(id), &value);
                    }
                }
            }
            if !t.fks.entries.is_empty() {
                out.set(format!("class {}", pyast::name_of(&node)), t);
            }
        } else if node.kind() == "call" && func_last(&node) == "Table" {
            let a = args(&node).pos;
            let Some(first) = a.first() else { continue };
            let name = col_ref(first).unwrap_or_else(|| "?".into());
            let mut t = Table::default();
            for x in &a[1..] {
                if x.kind() != "call" {
                    continue;
                }
                let xa = args(x).pos;
                if func_last(x) == "Column"
                    && let Some(col) = xa.first().and_then(constant_text)
                {
                    t.column(col, x);
                } else {
                    t.constraint(x);
                }
            }
            if !t.fks.entries.is_empty() {
                out.set(format!("table {name}"), t);
            }
        }
    }
    out
}

/// The value of a constant as Python would print it in a key (strings as themselves).
fn constant_text(n: &N) -> Option<String> {
    string(n).or_else(|| {
        matches!(&*n.kind(), "integer" | "float" | "true" | "false" | "none")
            .then(|| n.text().into_owned())
    })
}
