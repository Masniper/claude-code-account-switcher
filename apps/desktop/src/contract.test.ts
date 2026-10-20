import { describe, expect, it } from "vitest";
import json from "./contract.sample.json";
import library from "../../../src/gui.contract.json";
import { sample } from "./contract.sample";
import { errors } from "./api";

describe("contract", () => {
  it("the typed sample is exactly what the desktop crate's tests serialize", () => {
    expect(json).toStrictEqual(sample);
  });

  it("agrees with the library's own contract file, field for field", () => {
    // `revision` is the shell's ordering number, added around the library
    // snapshot; everything else must be exactly what `claude_acc::gui` sends.
    const { revision: _revision, ...snapshot } = sample.snapshot;
    expect(snapshot).toStrictEqual(library.snapshot);
    expect(sample.bindings).toStrictEqual(library.bindings);
    expect(sample.sessions).toStrictEqual(library.sessions);
    expect(sample.gui_errors).toStrictEqual(library.gui_errors);
  });

  it("every error code the backend can send has copy, and no other", () => {
    const codes = [...sample.gui_errors, ...sample.desktop_errors].sort();
    expect(Object.keys(errors).sort()).toStrictEqual(codes);
  });
});
