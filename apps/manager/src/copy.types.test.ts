import { expect, it } from "vitest";
import ts from "typescript";

// What `tsc` does and does not catch when a language table is written, as
// executable cases rather than prose. Each snippet is compiled against the
// real `Messages`; the expected diagnostic codes are what TypeScript reports
// today. If an upgrade changes them, this fails, and the table below is the
// thing to update.

// `src/`, with a trailing slash. Sliced from the URL by hand: there is no
// `@types/node` for `node:path`, and jsdom's `URL` resolves "." to "/src".
const src = decodeURIComponent(import.meta.url.replace(/^file:\/\//, "").replace(/[^/]*$/, ""));

const prelude = `
import { messages, type Messages } from "./i18n";
import type { IdentitySource } from "./contract";
const en = messages.en;
`;

const cases: [string, string, number[]][] = [
  // A control: if this reports anything, the harness is broken, not the table.
  ["a complete table", `export const t: Messages = { ...en };`, []],

  // Missing keys are always caught, wherever the table comes from. The code
  // depends on how many are missing.
  ["one key missing", `const { productName: _, ...t } = en; export const u: Messages = t;`, [2741]],
  [
    "two keys missing",
    `const { productName: _a, eyebrow: _b, ...t } = en; export const u: Messages = t;`,
    [2739],
  ],
  [
    "six keys missing",
    `const { productName: _a, productTagline: _b, eyebrow: _c, dialogEyebrow: _d,
       navHeading: _e, navAria: _f, ...t } = en;
     export const u: Messages = t;`,
    [2740],
  ],
  ["a string where a function belongs", `export const t: Messages = { ...en, sessionSize: "x" };`, [2322]],

  // An extra key is caught only when it is written in a literal that is
  // itself typed as `Messages`, whatever its value.
  ["an extra key in a typed literal", `export const t: Messages = { ...en, oops: "x" };`, [2353]],
  ["an extra object key in a typed literal", `export const t: Messages = { ...en, oops: {} };`, [2353]],
  [
    "an extra key inline in the messages record",
    `export const m: Record<"en", Messages> = { en: { ...en, oops: "x" } };`,
    [2353],
  ],
  [
    "an extra key in an untyped variable",
    `const t = { ...en, oops: "x" }; export const u: Messages = t;`,
    [],
  ],
  [
    "an extra key spread in from a fragment",
    `const extra = { oops: "x" }; export const t: Messages = { ...en, ...extra };`,
    [],
  ],
  [
    "an extra key through Object.assign",
    `export const t: Messages = Object.assign({}, en, { oops: "x" });`,
    [],
  ],

  // The sub-tables keyed by a contract union use `satisfies`. A cast instead
  // would accept a missing state and render `undefined` for it.
  [
    "satisfies, a state missing",
    `export const s = { live: "", local: "", offline: "", unknown: "" } satisfies Record<IdentitySource, string>;`,
    [1360],
  ],
  [
    "as, a state missing",
    `export const s = { live: "", local: "", offline: "", unknown: "" } as Record<IdentitySource, string>;`,
    [],
  ],
];

it("tsc catches these mistakes in a language table, and lets these through", () => {
  const config = ts.getParsedCommandLineOfConfigFile(
    `${src}../tsconfig.json`,
    {},
    { ...ts.sys, onUnRecoverableConfigFileDiagnostic: () => {} },
  );
  if (!config) throw new Error("tsconfig.json did not parse");

  // One program for every case: each snippet is its own virtual file in
  // `src`, so it imports `./i18n` exactly as a component would.
  const files = new Map(
    cases.map(([, code], i) => [`${src}__copy_case_${i}.ts`, prelude + code]),
  );
  const host = ts.createCompilerHost(config.options);
  const { getSourceFile, fileExists, readFile } = host;
  host.fileExists = (name) => files.has(name) || fileExists(name);
  host.readFile = (name) => files.get(name) ?? readFile(name);
  host.getSourceFile = (name, version, ...rest) => {
    const text = files.get(name);
    return text === undefined
      ? getSourceFile(name, version, ...rest)
      : ts.createSourceFile(name, text, version);
  };
  const program = ts.createProgram([...files.keys()], config.options, host);

  const got = cases.map(([name], i) => {
    const file = program.getSourceFile(`${src}__copy_case_${i}.ts`)!;
    return [name, program.getSemanticDiagnostics(file).map((d) => d.code)];
  });
  expect(got).toStrictEqual(cases.map(([name, , codes]) => [name, codes]));
});
