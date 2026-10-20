import { describe, expect, it } from "vitest";
import ts from "typescript";
import { sample } from "./contract.sample";
import { LANGS, messages } from "./i18n";

/** Every leaf of a table as `path: kind`, so two tables can be compared. */
function shape(table: object, prefix = ""): string[] {
  return Object.entries(table).flatMap(([key, value]) =>
    typeof value === "object" && value !== null
      ? shape(value, `${prefix}${key}.`)
      : [`${prefix}${key}: ${typeof value}`],
  );
}

describe("copy", () => {
  it("every language has exactly the English keys, with the same shapes", () => {
    // `tsc` enforces this already; this keeps the guarantee visible in the
    // test run as well, and names the key when it fails.
    for (const lang of LANGS) {
      expect(shape(messages[lang]).sort(), lang).toStrictEqual(
        shape(messages.en).sort(),
      );
    }
  });

  it("every error code the backend can send has copy, and no other", () => {
    const codes = [...sample.gui_errors, ...sample.manager_errors].sort();
    for (const lang of LANGS) {
      expect(Object.keys(messages[lang].errors).sort(), lang).toStrictEqual(codes);
    }
  });
});

// The components' source, as Vite sees it.
const components = import.meta.glob(["./**/*.tsx", "!./**/*.test.tsx", "!./i18n.tsx"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

// Attributes whose values are read by people; the rest (className, key, id,
// type, …) are identifiers.
const READ_BY_PEOPLE = new Set(["aria-label", "aria-valuetext", "title", "placeholder", "alt"]);
const words = /[A-Za-z]{2,}/;

function literalCopy(file: string, text: string): string[] {
  const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const found: string[] = [];
  const report = (node: ts.Node, value: string) => {
    const { line } = source.getLineAndCharacterOfPosition(node.getStart(source));
    found.push(`${file}:${line + 1} ${JSON.stringify(value.trim())}`);
  };
  // The attribute an expression sits in, if any: copy in `className={...}`
  // is a class name, in `title={...}` it is copy.
  const attributeOf = (node: ts.Node): string | null => {
    for (let n: ts.Node | undefined = node.parent; n; n = n.parent) {
      if (ts.isJsxAttribute(n)) return n.name.getText(source);
      if (ts.isJsxElement(n) || ts.isJsxSelfClosingElement(n)) return null;
    }
    return null;
  };
  // Whether a literal is what a JSX expression renders, rather than an
  // operand: `{busy ? "Working…" : x}` renders it, `{a.name === "default"}`
  // only compares with it.
  const rendered = (node: ts.Node): boolean => {
    const parent = node.parent;
    if (ts.isJsxExpression(parent)) return true;
    if (ts.isParenthesizedExpression(parent)) return rendered(parent);
    if (ts.isConditionalExpression(parent) && parent.condition !== node) return rendered(parent);
    if (
      ts.isBinaryExpression(parent) &&
      parent.right === node &&
      [ts.SyntaxKind.AmpersandAmpersandToken, ts.SyntaxKind.BarBarToken, ts.SyntaxKind.QuestionQuestionToken].includes(
        parent.operatorToken.kind,
      )
    ) {
      return rendered(parent);
    }
    return false;
  };
  // A template's own text, without the code inside `${…}`.
  const literalText = (node: ts.Node): string | null => {
    if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) return node.text;
    if (ts.isTemplateExpression(node)) {
      return node.head.text + node.templateSpans.map((span) => span.literal.text).join("");
    }
    return null;
  };
  const visit = (node: ts.Node) => {
    if (ts.isJsxText(node) && words.test(node.text)) report(node, node.text);
    if (
      ts.isJsxAttribute(node) &&
      node.initializer &&
      ts.isStringLiteral(node.initializer) &&
      READ_BY_PEOPLE.has(node.name.getText(source)) &&
      words.test(node.initializer.text)
    ) {
      report(node, node.initializer.text);
    }
    const text = literalText(node);
    if (text !== null && words.test(text) && rendered(node)) {
      const attribute = attributeOf(node);
      if (attribute === null || READ_BY_PEOPLE.has(attribute)) report(node, text);
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return found;
}

it("components render no copy of their own: it all comes from i18n.tsx", () => {
  const found = Object.entries(components).flatMap(([file, text]) =>
    literalCopy(file, text),
  );
  expect(found).toStrictEqual([]);
});
