import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { api } from "../api";
import type { Account } from "../contract";
import { sample } from "../contract.sample";
import { CopyProvider, describeError, messages } from "../i18n";
import { NoticeProvider } from "../notices";
import { BackupPage, backupMode, restoreChoice } from "./BackupPage";
import type { Opened, Plan } from "./types";

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  api: {
    chooseDirectory: vi.fn(),
    backupSurvey: vi.fn(),
    backupCreate: vi.fn(),
    backupOpen: vi.fn(),
    backupPreview: vi.fn(),
    backupRestore: vi.fn(),
    backupClose: vi.fn(),
    backupPending: vi.fn(),
    backupRollback: vi.fn(),
    chooseBackupSave: vi.fn(),
    chooseBackupFile: vi.fn(),
  },
}));

const t = messages.en;
const accounts: Account[] = sample.snapshot.accounts;

const opened: Opened = {
  file: "F:\\env.ccbackup",
  files: 3,
  created_at: 1_700_000_000,
  source_platform: "macos",
  source_home: "/Users/m",
  tool_version: "0.21.0",
  accounts: [
    { label: "default", source_dir: "/Users/m/.claude", files: 2, projects_only: false },
    { label: "work", source_dir: "/Users/m/.claude-switch/accounts/work", files: 1, projects_only: false },
  ],
  projects: [{ source_root: "/Users/m/work/app", files: 1 }],
  bindings: [{ dir: "/Users/m/work/app", account: "work" }],
  default_account: "work",
  suggested_from: "/Users/m/work",
  redactions: [{ path: "accounts/default/config/settings.json", field: "env.API_TOKEN" }],
  excluded: [{ path: "accounts/default/config/.credentials.json", reason: "credential" }],
};

const plan: Plan = {
  accounts: [{ from: "work", to: "work-mac", new: true, whole: true }],
  projects: [{ from: "/Users/m/work/app", to: "F:\\work\\app" }],
  left_out: [],
  counts: { create: 1, same: 0, conflict: 1, incompatible: 1 },
  files: [
    { archive: "a", dest: "C:\\Users\\m\\.claude\\CLAUDE.md", step: "conflict", why: null, other: null },
    { archive: "b", dest: "C:\\Users\\m\\.claude\\skills\\x", step: "create", why: null, other: null },
    { archive: "c", dest: "C:\\Users\\m\\.claude\\l", step: "incompatible", why: "symlink_on_windows", other: null },
  ],
  links: [{ dir: "F:\\work\\app", account: "work-mac", step: "create" }],
  default_account: null,
  notes: [
    { kind: "login_needed", account: "work-mac" },
    { kind: "project_dir_renamed", account: "work", from: "-Users-m-work-app", to: "F--work-app" },
  ],
  replace: false,
};

function renderPage() {
  return render(
    <CopyProvider lang="en">
      <NoticeProvider>
        <BackupPage accounts={accounts} />
      </NoticeProvider>
    </CopyProvider>,
  );
}

beforeEach(() => {
  vi.mocked(api.backupPending).mockResolvedValue(null);
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("what a form asks for", () => {
  const rows = (entries: [string, boolean, string][]) =>
    Object.fromEntries(entries.map(([k, on, to]) => [k, { on, to }]));

  it("sends renamed accounts as SRC=DST and unchanged ones as SRC", () => {
    const c = restoreChoice(
      "everything",
      rows([
        ["default", true, "default"],
        ["work", true, "work-mac"],
        ["off", false, "x"],
      ]),
      rows([["/Users/m/work/app", true, ""]]),
      [
        { from: "/Users/m/work", to: "F:\\work" },
        { from: "", to: "F:\\x" },
      ],
      "",
      true,
    );
    expect(c).toEqual({
      maps: ["/Users/m/work=F:\\work"],
      accounts: ["default", "work=work-mac"],
      all_accounts: false,
      projects: ["/Users/m/work/app"],
      reuse: false,
      select: [],
      replace: true,
    });
  });

  it("sends no account for projects only, and no project for accounts only", () => {
    const accounts = rows([["work", true, "work"]]);
    const projects = rows([["/p", true, "D:\\p"]]);
    // An account named here would be restored whole.
    expect(restoreChoice("projects", accounts, projects, [], "", false).accounts).toEqual([]);
    expect(restoreChoice("projects", accounts, projects, [], "", false).projects).toEqual([
      "/p=D:\\p",
    ]);
    expect(restoreChoice("accounts", accounts, projects, [], "", false).projects).toEqual([]);
  });

  it("reuse sends the selection one path per line and nothing else", () => {
    const c = restoreChoice(
      "reuse",
      rows([["work", true, "work"]]),
      rows([["/p", true, "D:\\other"]]),
      [],
      "skills/deploy\r\n\n  agents/review.md  \n",
      false,
    );
    expect(c.reuse).toBe(true);
    expect(c.accounts).toEqual([]);
    expect(c.select).toEqual(["skills/deploy", "agents/review.md"]);
  });

  it("names the three kinds of backup", () => {
    expect([backupMode(1, 0), backupMode(0, 2), backupMode(1, 1), backupMode(0, 0)]).toEqual([
      "accounts",
      "projects",
      "both",
      "nothing",
    ]);
  });
});

describe("backup failures", () => {
  it("are worded with the name they carry", () => {
    expect(describeError(t, { code: "backup_account_exists", subject: "work" })).toContain(
      "named work already exists",
    );
    expect(describeError(t, { code: "backup_wrong_passphrase", subject: null })).toBe(
      "Wrong passphrase.",
    );
  });

  it("an unknown backup code is unrecognised, not unreachable", () => {
    expect(describeError(t, { code: "backup_from_the_future", subject: null })).toBe(
      t.unrecognised,
    );
  });
});

describe("create", () => {
  it("previews the selection, then encrypts it only with a confirmed passphrase", async () => {
    vi.mocked(api.backupSurvey).mockResolvedValue({
      accounts: ["work"],
      projects: [],
      projects_only: false,
      files: 12,
      bytes: 3 * 1024 * 1024,
      empty_projects: [],
      excluded: [{ path: "accounts/work/config/.credentials.json", reason: "credential" }],
    });
    vi.mocked(api.backupCreate).mockResolvedValue({
      file: "F:\\b.ccbackup",
      files: 12,
      redactions: [{ path: "accounts/work/config/settings.json", field: "env.API_TOKEN" }],
    });
    renderPage();
    const create = screen.getByRole("button", { name: t.backupCreateAction });
    expect((create as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByLabelText("work"));
    expect(screen.getByText(t.backupModes.accounts)).toBeTruthy();
    fireEvent.click(screen.getAllByRole("button", { name: t.backupPreview })[0]);
    await screen.findByText("12 files, 3.0 MB");
    expect(api.backupSurvey).toHaveBeenCalledWith(["work"], false, []);
    expect((create as HTMLButtonElement).disabled).toBe(false);

    fireEvent.click(create);
    const dialog = within(screen.getByRole("dialog"));
    fireEvent.change(dialog.getByPlaceholderText(t.backupFilePlaceholder), {
      target: { value: "F:\\b.ccbackup" },
    });
    fireEvent.change(dialog.getByLabelText(t.backupPassphrase), { target: { value: "one" } });
    fireEvent.change(dialog.getByLabelText(t.backupPassphraseAgain), { target: { value: "two" } });
    fireEvent.click(dialog.getByRole("button", { name: t.backupCreateConfirm }));
    await dialog.findByRole("alert");
    expect(api.backupCreate).not.toHaveBeenCalled();

    fireEvent.change(dialog.getByLabelText(t.backupPassphraseAgain), { target: { value: "one" } });
    fireEvent.click(dialog.getByRole("button", { name: t.backupCreateConfirm }));
    await screen.findByText(t.backupWritten);
    expect(api.backupCreate).toHaveBeenCalledWith("F:\\b.ccbackup", ["work"], false, [], "one");
    expect(screen.getByText("env.API_TOKEN")).toBeTruthy();
  });

  it("a change after the preview asks for a new one", async () => {
    vi.mocked(api.backupSurvey).mockResolvedValue({
      accounts: ["work"],
      projects: [],
      projects_only: false,
      files: 1,
      bytes: 1,
      empty_projects: [],
      excluded: [],
    });
    renderPage();
    fireEvent.click(screen.getByLabelText("work"));
    fireEvent.click(screen.getAllByRole("button", { name: t.backupPreview })[0]);
    await screen.findByText(t.backupWillHold);
    const create = screen.getByRole("button", { name: t.backupCreateAction }) as HTMLButtonElement;
    expect(create.disabled).toBe(false);
    fireEvent.click(screen.getByLabelText("work"));
    expect(create.disabled).toBe(true);
  });
});

describe("restore", () => {
  async function openBackup() {
    vi.mocked(api.backupOpen).mockResolvedValue(opened);
    renderPage();
    fireEvent.change(screen.getByPlaceholderText(t.backupOpenPlaceholder), {
      target: { value: opened.file },
    });
    const pass = screen.getByPlaceholderText(t.backupPassphrase) as HTMLInputElement;
    fireEvent.change(pass, { target: { value: "secret" } });
    fireEvent.click(screen.getByRole("button", { name: t.backupOpenAction }));
    await screen.findByText(t.backupVerified(3));
  }

  it("opens once, then shows what the backup holds and starts a mapping row", async () => {
    await openBackup();
    expect(api.backupOpen).toHaveBeenCalledWith(opened.file, "secret");
    expect(screen.getByDisplayValue("/Users/m/work")).toBeTruthy();
    expect(screen.getByText("env.API_TOKEN")).toBeTruthy();
    // The passphrase field is gone with the open form; nothing keeps it.
    expect(screen.queryByPlaceholderText(t.backupPassphrase)).toBeNull();
  });

  it("restores exactly what was previewed, and only after a preview", async () => {
    vi.mocked(api.backupPreview).mockResolvedValue(plan);
    vi.mocked(api.backupRestore).mockResolvedValue({
      written: 1,
      replaced: 0,
      skipped: 2,
      switch_changes: 1,
      originals: null,
    });
    await openBackup();
    fireEvent.change(screen.getByLabelText(t.backupMapTo), { target: { value: "F:\\work" } });
    fireEvent.change(screen.getByLabelText(t.backupRestoreAs("work")), {
      target: { value: "work-mac" },
    });
    const restore = screen.getByRole("button", { name: t.backupRestoreAction }) as HTMLButtonElement;
    expect(restore.disabled).toBe(true);

    fireEvent.click(screen.getAllByRole("button", { name: t.backupPreview })[1]);
    await screen.findByText(t.backupPlanCounts(1, 0, 1, 1));
    const sent = vi.mocked(api.backupPreview).mock.calls[0][0];
    expect(sent.maps).toEqual(["/Users/m/work=F:\\work"]);
    expect(sent.accounts).toEqual(["default", "work=work-mac"]);
    // Conflicts are shown first, and the reason a file cannot be written.
    expect(screen.getByText("C:\\Users\\m\\.claude\\CLAUDE.md")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: t.backupStepFilter("incompatible", 1) }));
    expect(screen.getByText(t.backupWhy.symlink_on_windows)).toBeTruthy();
    expect(screen.getByText(t.backupLoginNeeded("work-mac"))).toBeTruthy();
    expect(restore.disabled).toBe(false);

    // A change after the preview: the old one no longer describes the restore.
    fireEvent.click(screen.getByLabelText(t.backupReplace));
    expect(restore.disabled).toBe(true);
    expect(screen.getByText(t.backupPlanStale)).toBeTruthy();
    fireEvent.click(screen.getByLabelText(t.backupReplace));
    expect(restore.disabled).toBe(false);

    fireEvent.click(restore);
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: t.backupRestoreConfirm }),
    );
    await screen.findByText(t.backupRestored);
    expect(api.backupRestore).toHaveBeenCalledWith(sent);
  });

  it("a failed open says why, and keeps the form", async () => {
    vi.mocked(api.backupOpen).mockRejectedValue({ code: "backup_wrong_passphrase", subject: null });
    renderPage();
    fireEvent.change(screen.getByPlaceholderText(t.backupOpenPlaceholder), {
      target: { value: "x" },
    });
    fireEvent.change(screen.getByPlaceholderText(t.backupPassphrase), { target: { value: "no" } });
    fireEvent.click(screen.getByRole("button", { name: t.backupOpenAction }));
    await screen.findByText("Wrong passphrase.");
    expect(screen.getByPlaceholderText(t.backupOpenPlaceholder)).toBeTruthy();
  });
});

describe("an unfinished restore", () => {
  it("is shown at the top and can be undone", async () => {
    vi.mocked(api.backupPending)
      .mockResolvedValueOnce("C:\\Users\\m\\.claude-switch\\restore-backups\\1")
      .mockResolvedValue(null);
    vi.mocked(api.backupRollback).mockResolvedValue("C:\\Users\\m\\.claude-switch\\restore-backups\\1");
    renderPage();
    await screen.findByText(t.backupPendingTitle);
    fireEvent.click(screen.getByRole("button", { name: t.backupRollbackAction }));
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: t.backupRollbackConfirm }),
    );
    await waitFor(() => expect(screen.queryByText(t.backupPendingTitle)).toBeNull());
    expect(api.backupRollback).toHaveBeenCalledOnce();
  });
});
