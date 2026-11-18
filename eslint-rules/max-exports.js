/**
 * Caps how many bindings one module exports, so a file that grows a wide
 * public surface is split by idea instead. Re-exports (`export ... from`) add
 * nothing of their own to a module and are not counted: they are what keeps a
 * barrel such as `src/api/index.ts` a one-line-per-type list.
 */
const DEFAULT_MAX = 10;

function declaredCount(declaration) {
  if (declaration.type === "VariableDeclaration") return declaration.declarations.length;
  return 1;
}

/** @type {import("eslint").Rule.RuleModule} */
const maxExports = {
  meta: {
    type: "suggestion",
    schema: [
      {
        type: "object",
        properties: { max: { type: "integer", minimum: 0 } },
        additionalProperties: false,
      },
    ],
    messages: {
      tooMany: "Module exports {{count}} bindings; the limit is {{max}}. Split it by idea.",
    },
  },
  create(context) {
    const max = context.options[0]?.max ?? DEFAULT_MAX;
    let count = 0;
    return {
      ExportNamedDeclaration(node) {
        if (node.source) return;
        count += node.declaration ? declaredCount(node.declaration) : node.specifiers.length;
      },
      ExportDefaultDeclaration() {
        count += 1;
      },
      "Program:exit"(program) {
        if (count > max) {
          context.report({ node: program, messageId: "tooMany", data: { count, max } });
        }
      },
    };
  },
};

export default { rules: { "max-exports": maxExports } };
