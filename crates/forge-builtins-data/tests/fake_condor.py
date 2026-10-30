"""A stand-in for condor 0.3.0 (PyPI cndr) for the hermetic pack examples of
python/no-dead-code: the same arguments (`--format json`, `--exclude a,b`, paths), the same
config (`[tool.condor]` / `[tool.vulture]` in ./pyproject.toml: paths, exclude, ignore_names,
min_confidence), the same JSON, confidences (imports 90, unreachable code 100, the rest 60)
and exit codes (0 nothing, 3 findings, 1 only files that do not parse, 2 bad options).

It is a name-based approximation, enough for small examples: a definition is used when its
name is loaded, or read as an attribute, anywhere in the scanned files. Decorated functions,
dunders, imports in `__init__.py` and names in `__all__` count as used.
"""

import ast
import fnmatch
import json
import os
import sys

try:
    import tomllib
except ImportError:  # Python < 3.11: no config
    tomllib = None

SKIP_DIRS = {".git", ".venv", "venv", "node_modules", "__pycache__"}


def usage(msg):
    sys.stderr.write(f"condor: error: {msg}\n")
    sys.exit(2)


def config():
    if tomllib is None or not os.path.isfile("pyproject.toml"):
        return {}
    with open("pyproject.toml", "rb") as f:
        try:
            tool = tomllib.load(f).get("tool", {})
        except tomllib.TOMLDecodeError:
            return {}
    return tool.get("condor") or tool.get("vulture") or {}


def as_list(v):
    if v is None:
        return []
    return [v] if isinstance(v, str) else list(v)


def files(paths, exclude):
    out = []
    for p in paths:
        p = os.path.abspath(p)
        if os.path.isfile(p):
            out.append(p)
            continue
        for d, dirs, names in os.walk(p):
            dirs[:] = sorted(x for x in dirs if x not in SKIP_DIRS)
            for n in sorted(names):
                if n.endswith((".py", ".pyi")):
                    out.append(os.path.join(d, n))
    return [f for f in out if not any(fnmatch.fnmatch(f, e) or e in f.split(os.sep) for e in exclude)]


def dunder(name):
    return name.startswith("__") and name.endswith("__")


class Defs(ast.NodeVisitor):
    def __init__(self, path):
        self.path = path
        self.defs = []  # (line, end_line, kind, name)
        self.unreachable = []  # (line, after)
        self.used = set()
        self.in_class = False

    def add(self, node, kind, name):
        self.defs.append((node.lineno, getattr(node, "end_lineno", node.lineno), kind, name))

    def body(self, stmts):
        for i, s in enumerate(stmts[:-1]):
            if isinstance(s, (ast.Return, ast.Raise, ast.Continue, ast.Break)):
                after = type(s).__name__.lower()
                self.unreachable.append((stmts[i + 1].lineno, stmts[-1].end_lineno, after))
                break

    def generic_visit(self, node):
        for field in ("body", "orelse", "finalbody"):
            stmts = getattr(node, field, None)
            if isinstance(stmts, list) and stmts and isinstance(stmts[0], ast.stmt):
                self.body(stmts)
        super().generic_visit(node)

    def visit_Import(self, node):
        if not self.path.endswith("__init__.py"):
            for a in node.names:
                self.add(node, "import", (a.asname or a.name).split(".")[0])

    def visit_ImportFrom(self, node):
        if not self.path.endswith("__init__.py"):
            for a in node.names:
                if a.name != "*":
                    self.add(node, "import", a.asname or a.name)

    def visit_FunctionDef(self, node):
        props = [d for d in node.decorator_list if isinstance(d, ast.Name) and d.id == "property"]
        if props:
            self.add(node, "property", node.name)
        elif not node.decorator_list and not dunder(node.name):
            self.add(node, "method" if self.in_class else "function", node.name)
        was, self.in_class = self.in_class, False
        self.generic_visit(node)
        self.in_class = was

    visit_AsyncFunctionDef = visit_FunctionDef

    def visit_ClassDef(self, node):
        if not node.decorator_list:
            self.add(node, "class", node.name)
        was, self.in_class = self.in_class, True
        self.generic_visit(node)
        self.in_class = was

    def visit_Assign(self, node):
        for t in node.targets:
            if isinstance(t, ast.Name) and not dunder(t.id):
                self.add(t, "variable", t.id)
            elif isinstance(t, ast.Attribute) and isinstance(t.ctx, ast.Store):
                self.add(t, "attribute", t.attr)
            if isinstance(t, ast.Name) and t.id == "__all__":
                for c in ast.walk(node.value):
                    if isinstance(c, ast.Constant) and isinstance(c.value, str):
                        self.used.add(c.value)
        self.generic_visit(node)

    def visit_Name(self, node):
        if not isinstance(node.ctx, ast.Store):
            self.used.add(node.id)

    def visit_Attribute(self, node):
        if not isinstance(node.ctx, ast.Store):
            self.used.add(node.attr)
        self.generic_visit(node)


def main(argv):
    fmt, exclude, paths = "text", None, []
    it = iter(argv)
    for a in it:
        if a == "--format":
            fmt = next(it, None) or usage("--format needs a value")
        elif a == "--exclude":
            exclude = [e for e in (next(it, None) or "").split(",") if e]
        elif a.startswith("-"):
            usage(f"unrecognized arguments: {a}")
        else:
            paths.append(a)
    cfg = config()
    paths = paths or as_list(cfg.get("paths"))
    if not paths:
        usage("please pass at least one file or directory")
    if exclude is None:
        exclude = as_list(cfg.get("exclude"))
    ignore = as_list(cfg.get("ignore_names"))
    floor = int(cfg.get("min_confidence", 0))
    scanned, broken = [], 0
    for f in files(paths, exclude):
        try:
            with open(f, encoding="utf-8") as h:
                tree = ast.parse(h.read(), filename=f)
        except SyntaxError as e:
            sys.stderr.write(f"{f}:{e.lineno or 1}: {e.msg}\n")
            broken += 1
            continue
        d = Defs(f)
        d.visit(tree)
        scanned.append(d)
    used = set().union(*(d.used for d in scanned)) if scanned else set()
    out = []
    for d in scanned:
        for line, end, kind, name in d.defs:
            if name in used or any(fnmatch.fnmatch(name, p) for p in ignore):
                continue
            conf = 90 if kind == "import" else 60
            out.append((d.path, line, end, kind, name, f"unused {kind} '{name}'", conf))
        for line, end, after in d.unreachable:
            out.append((d.path, line, end, "unreachable_code", after, f"unreachable code after '{after}'", 100))
    out = sorted(o for o in out if o[6] >= floor)
    if fmt == "json":
        keys = ("path", "line", "end_line", "kind", "name", "message", "confidence")
        rows = [dict(zip(keys, o), code="V000", size=o[2] - o[1] + 1) for o in out]
        print(json.dumps(rows, indent=2, sort_keys=True))
    else:
        for o in out:
            print(f"{o[0]}:{o[1]}: {o[5]} ({o[6]}% confidence)")
    if out:
        return 3
    return 1 if broken and not scanned else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
