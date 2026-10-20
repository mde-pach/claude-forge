import Ajv2020 from "ajv/dist/2020.js";

const ID = "^[a-z][a-z0-9-]*$";
const RULE_ID = "^[a-z][a-z0-9-]*/[a-z][a-z0-9-]*$";

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
      },
    },
  },
  if: { properties: { level: { const: "user" } } },
  then: { not: { required: ["detect"] } },
} as const;

const target = {
  type: "object",
  required: ["target"],
  properties: { target: { enum: ["tool", "ast-grep", "permission", "guard", "sandbox", "check"] } },
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
          mode: { enum: ["forbid", "no-increase", "no-decrease"] },
          rule: { type: "object", minProperties: 1 },
          constraints: { type: "object" },
          utils: { type: "object" },
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
        },
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
          when_changed: { type: "array", items: { type: "string", minLength: 1 }, minItems: 1 },
          requires_local_db: { type: "boolean" },
          timeout: { type: "integer", minimum: 1, maximum: 600 },
        },
      },
    },
  ],
} as const;

export const ruleSchema = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  $id: "forge:rule",
  type: "object",
  additionalProperties: false,
  required: ["id", "description", "scope", "enforce"],
  properties: {
    id: { type: "string", pattern: RULE_ID },
    description: { type: "string", minLength: 1, pattern: "^[^\\n\\t]+$" },
    scope: {
      type: "object",
      additionalProperties: false,
      required: ["language"],
      properties: {
        language: { enum: ["python", "typescript", "tsx", "any"] },
        files: { type: "array", items: { type: "string", minLength: 1 }, minItems: 1 },
      },
    },
    enforce: { type: "array", minItems: 1, items: target },
    examples: {
      type: "object",
      additionalProperties: false,
      properties: {
        match: { type: "array", items: { type: "string" } },
        no_match: { type: "array", items: { type: "string" } },
      },
    },
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
  },
} as const;

const ajv = new Ajv2020({ allErrors: true, strict: true, strictRequired: false, strictTypes: false, allowUnionTypes: true });
const validators = {
  pack: ajv.compile(packSchema),
  rule: ajv.compile(ruleSchema),
  project: ajv.compile(projectConfigSchema),
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
