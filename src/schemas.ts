import Ajv2020 from "ajv/dist/2020.js";
import { TARGET_KINDS, TARGETS } from "./targets/index.js";
import type { LockFile, PackFile, ProjectConfig, RuleFile } from "./types.js";
import { ECOSYSTEMS, HOOK_MANAGERS, LANGUAGES, LEVELS } from "./core/vocabulary.js";

const SEGMENT = "[a-z][a-z0-9-]*";
/** Pack ids are paths in the pack tree: "python", "python/sqlalchemy/alembic". */
const ID = `^${SEGMENT}(/${SEGMENT})*$`;
/** Rule ids are a pack id plus the rule name. */
const RULE_ID = `^${SEGMENT}(/${SEGMENT})+$`;
/** A file role (§15.4): `@<role>` in a glob list, `!@<role>` to exclude its files. */
const ROLE = "[a-z][a-z0-9-]*";
const roleGlobs = { type: "array", minItems: 1, items: { type: "string", minLength: 1 } } as const;

export const packSchema = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  $id: "forge:pack",
  type: "object",
  additionalProperties: false,
  required: ["id", "description", "level"],
  properties: {
    id: { type: "string", pattern: ID },
    description: { type: "string", minLength: 1 },
    level: { enum: LEVELS },
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
              ecosystem: { enum: ECOSYSTEMS },
              name: { type: "string", minLength: 1 },
              version: { type: "string", minLength: 1 },
            },
          },
        },
        /** The project is a library published to that ecosystem (packs.ts, isPyLibrary / isNpmLibrary). */
        library_any: { type: "array", items: { enum: ECOSYSTEMS }, uniqueItems: true },
        /** Active in every repository (a root pack not tied to a language). */
        always: { const: true },
        /** A file matching one of files has a line matching pattern (a regular expression): a literal value in a literal config file. */
        content_any: {
          type: "array",
          minItems: 1,
          items: {
            type: "object",
            additionalProperties: false,
            required: ["files", "pattern"],
            properties: {
              files: { type: "array", minItems: 1, items: { type: "string", minLength: 1 } },
              pattern: { type: "string", minLength: 1 },
            },
          },
        },
        /** A .yaml / .yml file whose first document is a mapping with all these top-level keys. */
        yaml_keys: { type: "array", minItems: 1, uniqueItems: true, items: { type: "string", minLength: 1 } },
      },
    },
    /**
     * Tool diagnostics this pack switches off where it is active: no rule's tool target asks
     * for them, and forge init writes Forge's tool config with them off (Biome).
     */
    tool_off: {
      type: "array",
      minItems: 1,
      items: {
        type: "object",
        additionalProperties: false,
        required: ["tool", "expects", "reason"],
        properties: {
          tool: { enum: ["biome"] },
          expects: { type: "string", pattern: "^lint/[a-zA-Z]+/[a-zA-Z]+$" },
          reason: { type: "string", minLength: 1 },
        },
      },
    },
  },
  if: { properties: { level: { const: "user" } } },
  then: { not: { required: ["detect"] } },
} as const;

/**
 * A where condition: the type the metavariable's expression has (§15.1), the declaration
 * the name it is bound to refers to (§15.6), or not.
 */
const typeSpec = {
  type: "object",
  additionalProperties: false,
  // where the type is declared: a package or a module, never only a name
  anyOf: [{ required: ["package"] }, { required: ["module"] }],
  properties: {
    package: {
      oneOf: [
        { type: "string", minLength: 1 },
        { type: "array", minItems: 1, items: { type: "string", minLength: 1 } },
      ],
    },
    module: {
      oneOf: [
        { type: "string", minLength: 1 },
        { type: "array", minItems: 1, items: { type: "string", minLength: 1 } },
      ],
    },
    name: {
      oneOf: [
        { type: "string", pattern: "^[A-Za-z_$][A-Za-z0-9_$]*$" },
        { type: "array", minItems: 1, items: { type: "string", pattern: "^[A-Za-z_$][A-Za-z0-9_$]*$" } },
      ],
    },
    unknown: { enum: ["match", "no-match"] },
  },
} as const;
/** A spec, or several: the condition holds when one of them does. */
const typeSpecs = { oneOf: [{ $ref: "#/$defs/spec" }, { type: "array", minItems: 1, items: { $ref: "#/$defs/spec" } }] } as const;
const whereCondition = {
  oneOf: [
    { type: "object", additionalProperties: false, required: ["type"], properties: { type: typeSpecs } },
    { type: "object", additionalProperties: false, required: ["declared"], properties: { declared: typeSpecs } },
    // the name is a use the checker reports as deprecated (§15.7)
    { type: "object", additionalProperties: false, required: ["deprecated"], properties: { deprecated: { const: true } } },
    { type: "object", additionalProperties: false, required: ["not"], properties: { not: { $ref: "#/$defs/where" } } },
  ],
} as const;

/** A target (§6.2): its kind, then the fields its kind's schema allows. */
const target = {
  type: "object",
  required: ["target"],
  properties: { target: { enum: TARGET_KINDS } },
  allOf: TARGET_KINDS.map((k) => ({ if: { properties: { target: { const: k } } }, then: TARGETS[k].schema })),
};

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
  $defs: { where: whereCondition, spec: typeSpec },
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
        language: { enum: LANGUAGES },
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
        // the packages the examples are type-checked against (rules with where, §15.1)
        deps: {
          type: "array",
          minItems: 1,
          items: {
            type: "object",
            additionalProperties: false,
            required: ["ecosystem", "name", "version"],
            properties: {
              ecosystem: { enum: ECOSYSTEMS },
              name: { type: "string", minLength: 1 },
              version: { type: "string", pattern: "^[0-9][0-9A-Za-z.+-]*$" },
            },
          },
        },
      },
    },
    review: { type: "string", minLength: 1, pattern: "^[^\\n\\t]+$" },
    // a version gate: every dependency declared, in its range, where the rule's pack applies
    when: {
      type: "object",
      additionalProperties: false,
      required: ["deps"],
      properties: {
        deps: {
          type: "array",
          minItems: 1,
          items: {
            type: "object",
            additionalProperties: false,
            required: ["ecosystem", "name", "version"],
            properties: { ecosystem: { enum: ECOSYSTEMS }, name: { type: "string", minLength: 1 }, version: { type: "string", minLength: 1 } },
          },
        },
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
    files: {
      type: "object",
      propertyNames: { pattern: `^${ROLE}$` },
      additionalProperties: {
        type: "object",
        additionalProperties: false,
        minProperties: 1,
        properties: { add: roleGlobs, set: roleGlobs },
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
    roles: { type: "object", propertyNames: { pattern: `^${ROLE}$` }, additionalProperties: { type: "array", items: { type: "string" } } },
    blocks: {
      type: "array",
      items: {
        type: "object",
        additionalProperties: false,
        required: ["file", "kind", "created", "hash"],
        properties: {
          file: { type: "string", minLength: 1 },
          kind: { enum: HOOK_MANAGERS },
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

/** What each schema validates, as its TypeScript type. */
interface Validated {
  pack: PackFile;
  rule: RuleFile;
  project: ProjectConfig;
  lock: LockFile;
}

/** The top-level property names of a schema. */
type SchemaKeys<S> = S extends { properties: infer P } ? keyof P : never;
type Same<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false;
/**
 * The schemas and the types name the same fields: a field added to one and not the other
 * fails the typecheck here.
 */
const schemasMatchTypes: [
  Same<SchemaKeys<typeof packSchema>, keyof PackFile>,
  Same<SchemaKeys<typeof ruleSchema>, keyof RuleFile>,
  Same<SchemaKeys<typeof projectConfigSchema>, keyof ProjectConfig>,
  Same<SchemaKeys<typeof lockSchema>, keyof LockFile>,
] = [true, true, true, true];
void schemasMatchTypes;

/** Validates data against a schema, and narrows it to that schema's type; throws with every error found. */
export function validate<K extends keyof Validated>(kind: K, data: unknown, source: string): asserts data is Validated[K] {
  const v = validators[kind];
  if (!v(data)) {
    const msgs = (v.errors ?? [])
      .filter((e) => e.keyword !== "if")
      .map((e) => `${e.instancePath || "/"} ${e.message}${e.params && "additionalProperty" in e.params ? ` (${e.params.additionalProperty})` : ""}`);
    throw new Error(`${source}: invalid ${kind}: ${[...new Set(msgs)].join("; ")}`);
  }
}
