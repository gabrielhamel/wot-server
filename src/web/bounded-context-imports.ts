import { dirname, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { definePlugin, defineRule } from "vite-plus/lint/plugins";
import type { ESTree } from "vite-plus/lint/plugins";

const sourceRoot = fileURLToPath(new URL("../", import.meta.url));
const boundedContexts = new Set([
  "accounts",
  "battles",
  "player-administration",
  "player-sessions",
  "social",
]);

const publishedContextImport = defineRule({
  meta: {
    messages: { useEntryPoint: "Import the {{owner}} web entry point." },
  },
  create(context) {
    const filename = context.physicalFilename;
    const importingContext = relative(sourceRoot, filename).split(sep, 1)[0];

    function check(source: ESTree.Expression | null | undefined) {
      if (source?.type !== "Literal" || typeof source.value !== "string") return;
      const specifier = source.value;
      if (!specifier.startsWith("./") && !specifier.startsWith("../")) return;
      const [owner, ...module] = relative(
        sourceRoot,
        resolve(dirname(filename), specifier.split(/[?#]/, 1)[0]),
      ).split(sep);
      if (!boundedContexts.has(owner) || importingContext === owner) return;
      if (
        module.length === 2 &&
        module[0] === "src" &&
        (module[1] === "web" || module[1] === "web.ts")
      )
        return;
      context.report({ node: source, messageId: "useEntryPoint", data: { owner } });
    }

    return {
      ImportDeclaration: (node) => check(node.source),
      ExportNamedDeclaration: (node) => check(node.source),
      ExportAllDeclaration: (node) => check(node.source),
      ImportExpression: (node) => check(node.source),
      TSImportType: (node) => check(node.source),
      TSExternalModuleReference: (node) => check(node.expression),
    };
  },
});

export default definePlugin({
  meta: { name: "web" },
  rules: { "published-context-import": publishedContextImport },
});
