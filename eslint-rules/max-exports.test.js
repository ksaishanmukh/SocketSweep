import { RuleTester } from "eslint";
import tseslint from "typescript-eslint";
import { describe, it } from "vitest";
import plugin from "./max-exports.js";

RuleTester.describe = describe;
RuleTester.it = it;

const names = (n) => Array.from({ length: n }, (_, i) => `export const a${i} = ${i};`).join("\n");

new RuleTester({ languageOptions: { parser: tseslint.parser } }).run(
  "max-exports",
  plugin.rules["max-exports"],
  {
    valid: [
      { code: names(3), options: [{ max: 3 }] },
      { code: "const a = 1, b = 2; export { a, b };", options: [{ max: 2 }] },
      { code: "export { a, b, c } from './x'; export * from './y';", options: [{ max: 1 }] },
      { code: "export type { A, B, C } from './x';", options: [{ max: 1 }] },
      { code: "export const a = 1;\nexport type T = number;", options: [{ max: 2 }] },
    ],
    invalid: [
      { code: names(4), options: [{ max: 3 }], errors: [{ messageId: "tooMany" }] },
      {
        code: "export function f() {}\nexport interface I {}\nexport enum E {}",
        options: [{ max: 2 }],
        errors: [{ messageId: "tooMany" }],
      },
      {
        code: "const a = 1, b = 2; export { a, b }; export default a;",
        options: [{ max: 2 }],
        errors: [{ messageId: "tooMany" }],
      },
      { code: names(11), errors: [{ messageId: "tooMany" }] },
    ],
  },
);
