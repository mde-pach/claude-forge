import Ajv2020 from "ajv/dist/2020.js";

const SEGMENT = "[a-z][a-z0-9-]*";
/** Pack ids are paths in the pack tree: "python", "python/sqlalchemy/alembic". */
const ID = `^${SEGMENT}(/${SEGMENT})*$`;
/** Rule ids are a pack id plus the rule name. */
const RULE_ID = `^${SEGMENT}(/${SEGMENT})+$`;

export const packSchema = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  $id: "forge:pack",
  type: "object",
  additionalProperties: false,
  required: ["id", "description", "level"],
  properties: {
    id: { type: "string", pattern: ID },
    description: { type: "string", minLength: 1 },
    level: { enum: ["user", "project"] },
    requires: { type: "array", items: { type: "string", pattern: ID }, uniqueItems: true },
    detect: {
      type: "object",
      additionalProperties: false,
      properties: {
        files_any: { type: "array", items: { type: "string", minLength: 1 } },
        deps_any: {
          type: "array",
          items: {
            type: "object",
            additionalProperties: false,
            required: ["ecosystem", "name"],
            properties: {
              ecosystem: { enum: ["pypi", "npm"] },
              name: { type: "string", minLength: 1 },
              version: { type: "string", minLength: 1 },
            },
          },
        },
        /** The project is a library published to that ecosystem (packs.ts, isPyLibrary / isNpmLibrary). */
        library_any: { type: "array", items: { enum: ["pypi", "npm"] }, uniqueItems: true },
      },
    },
  },
  if: { properties: { level: { const: "user" } } },
  then: { not: { required: ["detect"] } },
} as const;

const target = {
  type: "object",
  required: ["target"],
  properties: { target: { enum: ["tool", "ast-grep", "permission", "guard", "sandbox", "check", "grep"] } },
  allOf: [
    {
      if: { properties: { target: { const: "tool" } } },
      then: {
        additionalProperties: false,
        required: ["tool", "expects", "snippet"],
        properties: {
          target: true,
          tool: { enum: ["biome", "ruff", "basedpyright", "tsc"] },
          expects: { type: "string", minLength: 1 },
          snippet: { type: "string", minLength: 1 },
        },
      },
    },
    {
      if: { properties: { target: { const: "ast-grep" } } },
      then: {
        additionalProperties: false,
        required: ["mode", "rule"],
        properties: {
          target: true,
          mode: { enum: ["forbid", "no-increase", "no-decrease", "advise"] },
          rule: { type: "object", minProperties: 1 },
          constraints: { type: "object" },
          utils: { type: "object" },
        },
      },
    },
    {
      if: { properties: { target: { const: "grep" } } },
      then: {
        additionalProperties: false,
        required: ["mode", "pattern"],
        properties: {
          target: true,
          mode: { enum: ["forbid", "no-increase", "no-decrease"] },
          pattern: { type: "string", minLength: 1, pattern: "^[^\\t\\n]+$" },
          ignore_case: { type: "boolean" },
        },
      },
    },
    {
      if: { properties: { target: { const: "permission" } } },
      then: {
        additionalProperties: false,
        required: ["decision", "rule"],
        properties: {
          target: true,
          decision: { enum: ["deny", "ask"] },
          rule: { type: "string", pattern: "^[A-Za-z]+(\\(.+\\))?$" },
        },
      },
    },
    {
      if: { properties: { target: { const: "guard" } } },
      then: {
        additionalProperties: false,
        required: ["decision", "pattern"],
        properties: {
          target: true,
          decision: { enum: ["deny", "ask"] },
          pattern: { type: "string", minLength: 1, pattern: "^[^\\t\\n]+$" },
          check: { enum: ["ephemeral-exec"] },
        },
        // a built-in check decides which matching commands to ask about: a deny would refuse them all
        if: { required: ["check"] },
        then: { properties: { decision: { const: "ask" } } },
      },
    },
    {
      if: { properties: { target: { const: "sandbox" } } },
      then: {
        additionalProperties: false,
        required: ["path", "value"],
        properties: {
          target: true,
          path: { type: "string", pattern: "^[A-Za-z][A-Za-z0-9]*(\\.[A-Za-z][A-Za-z0-9]*)*$" },
          value: true,
        },
      },
    },
    {
      if: { properties: { target: { const: "check" } } },
      then: {
        additionalProperties: false,
        required: ["stage", "run"],
        properties: {
          target: true,
          stage: { enum: ["format", "lint", "typecheck", "migration", "test"] },
          run: { type: "string", minLength: 1 },
          changed_run: { type: "string", minLength: 1 },
          fix_run: { type: "string", minLength: 1, pattern: "\\{files\\}" },
          format_stdin: { type: "string", minLength: 1, pattern: "\\{file\\}" },
          rerun: { type: "string", minLength: 1, pattern: "\\{files\\}" },
          when_changed: { type: "array", items: { type: "string", minLength: 1 }, minItems: 1 },
          requires_local_db: { type: "boolean" },
          timeout: { type: "integer", minimum: 1, maximum: 600 },
        },
        allOf: [
          // rerun re-runs failing tests (§10.6); format_stdin formats base copies (§10.1)
          { if: { required: ["rerun"] }, then: { properties: { stage: { const: "test" } } } },
          { if: { required: ["format_stdin"] }, then: { properties: { stage: { enum: ["format", "lint"] } } } },
        ],
      },
    },
  ],
} as const;

/** A string (code in the rule's scope) or a fixture: files committed at HEAD, then the files of the change. */
const files = { type: "object", minProperties: 1, additionalProperties: { type: "string" }, propertyNames: { pattern: "^(?!/)(?!(.*/)?\\.\\.(/|$)).*[^/]$" } } as const;
const example = {
  oneOf: [
    { type: "string" },
    { type: "object", additionalProperties: false, required: ["files"], properties: { files, head: files } },
    { type: "object", additionalProperties: false, required: ["command"], properties: { command: { type: "string", minLength: 1 }, files } },
  ],
} as const;

export const ruleSchema = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  $id: "forge:rule",
  type: "object",
  additionalProperties: false,
  required: ["id", "description", "scope", "enforce"],
  // review is the question for an ask nobody can answer (§9.3): a guard ask, or the test-edit check's ask
  if: { required: ["review"] },
  then: {
    properties: {
      enforce: {
        contains: {
          anyOf: [
            { properties: { target: { const: "guard" }, decision: { const: "ask" } }, required: ["decision"] },
            { properties: { target: { const: "check" }, run: { pattern: "^\\s*forge_test_edits\\s*$" } }, required: ["run"] },
          ],
        },
      },
    },
  },
  properties: {
    id: { type: "string", pattern: RULE_ID },
    description: { type: "string", minLength: 1, pattern: "^[^\\n\\t]+$" },
    scope: {
      type: "object",
      additionalProperties: false,
      required: ["language"],
      properties: {
        language: { enum: ["python", "typescript", "tsx", "yaml", "any"] },
        files: { type: "array", items: { type: "string", minLength: 1 }, minItems: 1 },
      },
    },
    enforce: { type: "array", minItems: 1, items: target },
    examples: {
      type: "object",
      additionalProperties: false,
      properties: {
        match: { type: "array", items: example },
        no_match: { type: "array", items: example },
      },
    },
    review: { type: "string", minLength: 1, pattern: "^[^\\n\\t]+$" },
    deprecated: {
      type: "object",
      additionalProperties: false,
      required: ["replaced_by"],
      properties: { replaced_by: { type: "string", pattern: RULE_ID } },
    },
  },
} as const;

export const projectConfigSchema = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  $id: "forge:project",
  type: "object",
  additionalProperties: false,
  required: ["forge"],
  properties: {
    forge: { type: "string", pattern: "^\\d+\\.\\d+$" },
    packs: {
      type: "object",
      additionalProperties: false,
      properties: {
        add: { type: "array", items: { type: "string", pattern: ID } },
        remove: { type: "array", items: { type: "string", pattern: ID } },
      },
    },
    disable: {
      type: "array",
      items: {
        type: "object",
        additionalProperties: false,
        required: ["id", "reason"],
        properties: { id: { type: "string", pattern: RULE_ID }, reason: { type: "string", minLength: 1 } },
      },
    },
    accept: {
      type: "array",
      items: {
        type: "object",
        additionalProperties: false,
        required: ["rule", "change", "reason"],
        properties: {
          rule: { type: "string", pattern: RULE_ID },
          change: { type: "string", minLength: 1, pattern: "^[^\\n]+$" },
          reason: { type: "string", minLength: 1 },
        },
      },
    },
  },
} as const;

const SHA256 = "^[0-9a-f]{64}$";

/**
 * The lock (§12). Its paths are checked further by lock.ts (relative, inside the
 * repository, and only where Forge writes): `remove` deletes and `apply` writes them.
 */
export const lockSchema = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  $id: "forge:lock",
  type: "object",
  additionalProperties: false,
  required: ["forge_version", "claude_code_min", "rules", "files", "settings"],
  properties: {
    forge_version: { type: "string" },
    claude_code_min: { type: "string" },
    rules: { type: "object", propertyNames: { pattern: RULE_ID }, additionalProperties: { type: "string", pattern: SHA256 } },
    files: { type: "object", propertyNames: { minLength: 1 }, additionalProperties: { type: "string", pattern: SHA256 } },
    settings: {
      type: "array",
      items: {
        type: "object",
        additionalProperties: false,
        required: ["file", "path", "value"],
        properties: { file: { type: "string", minLength: 1 }, path: { type: "string", minLength: 1 }, value: true },
      },
    },
    blocks: {
      type: "array",
      items: {
        type: "object",
        additionalProperties: false,
        required: ["file", "kind", "created", "hash"],
        properties: {
          file: { type: "string", minLength: 1 },
          kind: { enum: ["pre-commit", "husky", "lefthook", "hooks"] },
          created: { type: "boolean" },
          hash: { type: "string", pattern: SHA256 },
        },
      },
    },
  },
} as const;

const ajv = new Ajv2020({ allErrors: true, strict: true, strictRequired: false, strictTypes: false, allowUnionTypes: true });
const validators = {
  pack: ajv.compile(packSchema),
  rule: ajv.compile(ruleSchema),
  project: ajv.compile(projectConfigSchema),
  lock: ajv.compile(lockSchema),
};

export function validate(kind: keyof typeof validators, data: unknown, source: string): void {
  const v = validators[kind];
  if (!v(data)) {
    const msgs = (v.errors ?? [])
      .filter((e) => e.keyword !== "if")
      .map((e) => `${e.instancePath || "/"} ${e.message}${e.params && "additionalProperty" in e.params ? ` (${e.params.additionalProperty})` : ""}`);
    throw new Error(`${source}: invalid ${kind}: ${[...new Set(msgs)].join("; ")}`);
  }
}
