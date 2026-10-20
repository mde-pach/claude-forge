import { parseDocument, visit, isScalar, isMap, isSeq, stringify } from "yaml";

/** Strict YAML: rejects duplicate keys, anchors, aliases and explicit tags. */
export function parseStrictYaml(text: string, source: string): unknown {
  const doc = parseDocument(text, { uniqueKeys: true, schema: "core", prettyErrors: true });
  const problems = [...doc.errors, ...doc.warnings].map((e) => e.message);
  visit(doc, {
    Alias() {
      problems.push("aliases are not allowed");
    },
    Node(_key, node) {
      if ((isScalar(node) || isMap(node) || isSeq(node)) && node.anchor) problems.push(`anchor &${node.anchor} is not allowed`);
      if ((isScalar(node) || isMap(node) || isSeq(node)) && node.tag) problems.push(`tag ${node.tag} is not allowed`);
    },
  });
  if (problems.length) throw new Error(`${source}: ${problems.join("; ")}`);
  return doc.toJS();
}

export function toYaml(value: unknown): string {
  return stringify(value, { lineWidth: 0, aliasDuplicateObjects: false });
}
