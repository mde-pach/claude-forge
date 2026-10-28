/**
 * kubeconform, Forge's own pinned Kubernetes schema validator (§13.5 release binary, §16.2
 * pinned tools): one range, the pin (RELEASE_BINARIES.kubeconform), proven on 0.8.0. Forge's
 * invocation (kubernetes.sh forge_kubeconform): `-strict -output json -cache <dir>
 * -schema-location default -schema-location <CRDs catalog URL> <files | ->`. Exit: 0 every
 * document valid, 1 an invalid document, a document it could not validate (no schema, schema
 * download failed, unparsable YAML) and a usage error alike; the JSON (stdout) says which. No
 * `--version`: `kubeconform -v`.
 */
import { RELEASE_BINARIES } from "../util.js";
import type { ToolAdapter } from "./types.js";

const PIN = RELEASE_BINARIES.kubeconform!.version;

export const KUBECONFORM: ToolAdapter = {
  tool: "kubeconform",
  pinned: PIN,
  verified: PIN,
  // forge_release_bin kubeconform
  runner: "forge_pinned kubeconform",
  versionArgs: "-v",
  usage: { exit: 1, pattern: "^(flag provided but not defined: |invalid (boolean )?value |.outputFormat. must be )" },
  ranges: [
    {
      from: PIN,
      source: `kubeconform ${PIN}: https://github.com/yannh/kubeconform/releases/tag/v${PIN}; kubeconform -h`,
      ops: {
        // caller: <cache dir> <files | ->  (FORGE_KUBECONFORM_CRDS is the second schema location)
        validate:
          "-strict -output json -schema-location default -schema-location https://raw.githubusercontent.com/datreeio/CRDs-catalog/main/{{.Group}}/{{.ResourceKind}}_{{.ResourceAPIVersion}}.json -cache",
      },
    },
  ],
};
