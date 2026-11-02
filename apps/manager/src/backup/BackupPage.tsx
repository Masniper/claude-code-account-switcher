import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "../api";
import { Dialog, Failure, Icon } from "../components";
import type { Account } from "../contract";
import { list, megabytes, useCopy } from "../i18n";
import { useNotices } from "../notices";
import type {
  Applied,
  Created,
  FileStep,
  Left,
  Opened,
  Plan,
  RestoreChoice,
  Step,
  Survey,
} from "./types";

/** File rows rendered per step; a whole account can hold thousands. */
export const FILE_PAGE = 200;

export function BackupPage({ accounts }: { accounts: Account[] }) {
  return (
    <>
      <PendingRestore />
      <CreatePanel accounts={accounts} />
      <RestorePanel />
    </>
  );
}

// ---------------------------------------------------------------------------
// An unfinished restore

function PendingRestore() {
  const t = useCopy();
  const { info } = useNotices();
  const [pending, setPending] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const load = useCallback(() => {
    api.backupPending().then(setPending, () => setPending(null));
  }, []);
  useEffect(load, [load]);
  if (!pending) return null;
  return (
    <div className="banner error" role="alert">
      <strong>{t.backupPendingTitle}</strong>
      <span>{t.backupPendingBody(pending)}</span>
      <button onClick={() => setConfirming(true)}>{t.backupRollbackAction}</button>
      {confirming && (
        <Dialog
          title={t.backupRollbackTitle}
          confirmLabel={t.backupRollbackConfirm}
          onConfirm={async () => {
            await api.backupRollback();
            info(t.backupRolledBack);
            load();
          }}
          onClose={() => setConfirming(false)}
        >
          <p>{t.backupRollbackBody}</p>
        </Dialog>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Create

type Mode = "accounts" | "projects" | "both" | "nothing";

/** Which of the three kinds of backup a selection makes. */
export function backupMode(accounts: number, projects: number): Mode {
  if (accounts && projects) return "both";
  if (accounts) return "accounts";
  if (projects) return "projects";
  return "nothing";
}

function CreatePanel({ accounts }: { accounts: Account[] }) {
  const t = useCopy();
  const { fail } = useNotices();
  const names = accounts.map((a) => a.name);
  const [chosen, setChosen] = useState<string[]>([]);
  const [projects, setProjects] = useState<string[]>([]);
  const [survey, setSurvey] = useState<Survey | null>(null);
  const [created, setCreated] = useState<Created | null>(null);
  const [creating, setCreating] = useState(false);
  const [busy, setBusy] = useState(false);

  // Any change makes the last preview stale; creating needs a fresh one.
  useEffect(() => setSurvey(null), [chosen, projects]);

  const mode = backupMode(chosen.length, projects.length);
  const toggle = (name: string) =>
    setChosen((c) => (c.includes(name) ? c.filter((n) => n !== name) : [...c, name]));

  const preview = async () => {
    setBusy(true);
    setCreated(null);
    try {
      setSurvey(await api.backupSurvey(chosen, false, projects));
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="project-picker backup-panel">
      <div className="panel-heading">
        <div className="summary-icon">
          <Icon name="backup" />
        </div>
        <div>
          <h2>{t.backupCreateTitle}</h2>
          <p>{t.backupCreateLead}</p>
        </div>
      </div>

      <fieldset>
        <legend>{t.backupAccountsLegend}</legend>
        <div className="choice-list">
          {names.map((name) => (
            <label key={name} className="check">
              <input
                type="checkbox"
                checked={chosen.includes(name)}
                onChange={() => toggle(name)}
              />
              {name === "default" ? t.standardAccount : name}
            </label>
          ))}
          <button
            type="button"
            className="quiet"
            onClick={() => setChosen(chosen.length === names.length ? [] : names)}
          >
            {chosen.length === names.length ? t.backupSelectNone : t.backupSelectAll}
          </button>
        </div>
      </fieldset>

      <fieldset>
        <legend>{t.backupProjectsLegend}</legend>
        {projects.length > 0 && (
          <div className="binding-list">
            {projects.map((p) => (
              <div className="binding-row" key={p}>
                <Icon name="projects" />
                <div>
                  <strong className="path">{p}</strong>
                </div>
                <button
                  type="button"
                  onClick={() => setProjects((ps) => ps.filter((x) => x !== p))}
                  aria-label={t.backupRemoveProjectAria(p)}
                >
                  {t.remove}
                </button>
              </div>
            ))}
          </div>
        )}
        <button
          type="button"
          disabled={busy}
          onClick={async () => {
            const path = await api.chooseDirectory();
            if (path && !projects.includes(path)) setProjects((ps) => [...ps, path]);
          }}
        >
          {t.backupAddProject}
        </button>
      </fieldset>

      <p className="backup-mode" role="status">
        {t.backupModes[mode]}
      </p>

      <div className="binding-actions">
        <button type="button" disabled={busy || mode === "nothing"} onClick={preview}>
          {busy ? t.working : t.backupPreview}
        </button>
        <button
          type="button"
          className="primary"
          disabled={busy || survey == null}
          onClick={() => setCreating(true)}
        >
          {t.backupCreateAction}
        </button>
      </div>

      {survey && <SurveySummary survey={survey} />}
      {created && <CreatedSummary created={created} />}

      {creating && survey && (
        <CreateDialog
          survey={survey}
          onCreate={async (file, passphrase) =>
            setCreated(await api.backupCreate(file, chosen, false, projects, passphrase))
          }
          onClose={() => setCreating(false)}
        />
      )}
    </section>
  );
}

function SurveySummary({ survey }: { survey: Survey }) {
  const t = useCopy();
  return (
    <div className="effective">
      <span className="eyebrow">{t.backupWillHold}</span>
      <h3>{t.backupFilesAndSize(survey.files, megabytes(t, survey.bytes))}</h3>
      <p>{t.backupWholeAccounts(survey.accounts.length ? list(t, survey.accounts) : "—")}</p>
      {survey.projects.map((p) => (
        <p key={p} className="path">
          {t.backupProjectLine(p)}
        </p>
      ))}
      {survey.projects_only && <p>{t.backupProjectsOnlyNote}</p>}
      {survey.empty_projects.map((p) => (
        <p key={p} className="warning">
          {t.backupEmptyProject(p)}
        </p>
      ))}
      <LeftOut excluded={survey.excluded} />
    </div>
  );
}

function LeftOut({ excluded }: { excluded: Left[] }) {
  const t = useCopy();
  if (!excluded.length) return null;
  return (
    <details>
      <summary>{t.backupLeftOut(excluded.length)}</summary>
      <ul className="file-list">
        {excluded.map((e) => (
          <li key={e.path}>
            <span className="badge">{t.backupReasons[e.reason]}</span>
            <span className="path">{e.path}</span>
          </li>
        ))}
      </ul>
    </details>
  );
}

function CreatedSummary({ created }: { created: Created }) {
  const t = useCopy();
  return (
    <div className="effective" role="status">
      <span className="eyebrow">{t.backupWritten}</span>
      <h3 className="path">{created.file}</h3>
      <p>{t.backupWrittenBody(created.files)}</p>
      {created.redactions.length > 0 && (
        <details open>
          <summary>{t.backupRedacted(created.redactions.length)}</summary>
          <ul className="file-list">
            {created.redactions.map((r) => (
              <li key={`${r.path}:${r.field}`}>
                <span className="path">{r.path}</span> <code>{r.field}</code>
              </li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}

function CreateDialog({
  survey,
  onCreate,
  onClose,
}: {
  survey: Survey;
  onCreate: (file: string, passphrase: string) => Promise<void>;
  onClose: () => void;
}) {
  const t = useCopy();
  const [file, setFile] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [again, setAgain] = useState("");
  const mismatch = again.length > 0 && again !== passphrase;
  return (
    <Dialog
      title={t.backupCreateTitle}
      confirmLabel={t.backupCreateConfirm}
      onConfirm={async () => {
        // Checked here as well as shown, so a mismatch can never be sent.
        if (passphrase !== again) throw { code: "backup_passphrase_mismatch", subject: null };
        await onCreate(file, passphrase);
      }}
      onClose={onClose}
    >
      <p>{t.backupCreateBody(survey.files, megabytes(t, survey.bytes))}</p>
      <label>
        {t.backupFileLabel}
        <span className="inline-field">
          <input
            value={file}
            onChange={(e) => setFile(e.target.value)}
            placeholder={t.backupFilePlaceholder}
            required
          />
          <button
            type="button"
            onClick={async () => {
              const path = await api.chooseBackupSave();
              if (path) setFile(path);
            }}
          >
            {t.browse}
          </button>
        </span>
      </label>
      <label>
        {t.backupPassphrase}
        <input
          type="password"
          autoComplete="new-password"
          value={passphrase}
          onChange={(e) => setPassphrase(e.target.value)}
          required
        />
      </label>
      <label>
        {t.backupPassphraseAgain}
        <input
          type="password"
          autoComplete="new-password"
          value={again}
          onChange={(e) => setAgain(e.target.value)}
          aria-invalid={mismatch}
          required
        />
      </label>
      {mismatch && <p className="field-error">{t.backupPassphraseMismatch}</p>}
      <p>{t.backupPassphraseWarning}</p>
    </Dialog>
  );
}

// ---------------------------------------------------------------------------
// Restore

type RestoreMode = "everything" | "accounts" | "projects" | "reuse";
const RESTORE_MODES: RestoreMode[] = ["everything", "accounts", "projects", "reuse"];

type Row = { on: boolean; to: string };
type Mapping = { from: string; to: string };

/**
 * What the form asks for, in the library's own terms. Projects-only sends
 * no account, which the library reads as "sessions into the accounts of
 * the same name"; naming one there would restore it whole.
 */
export function restoreChoice(
  mode: RestoreMode,
  accounts: Record<string, Row>,
  projects: Record<string, Row>,
  maps: Mapping[],
  select: string,
  replace: boolean,
): RestoreChoice {
  const pairs = (rows: Record<string, Row>) =>
    Object.entries(rows)
      .filter(([, r]) => r.on)
      .map(([from, r]) => (r.to.trim() && r.to.trim() !== from ? `${from}=${r.to.trim()}` : from));
  const withAccounts = mode === "everything" || mode === "accounts";
  const withProjects = mode !== "accounts";
  return {
    maps: maps
      .filter((m) => m.from.trim() && m.to.trim())
      .map((m) => `${m.from.trim()}=${m.to.trim()}`),
    accounts: withAccounts ? pairs(accounts) : [],
    all_accounts: false,
    projects: withProjects ? pairs(projects) : [],
    reuse: mode === "reuse",
    select:
      mode === "reuse"
        ? select
            .split(/\r?\n/)
            .map((s) => s.trim())
            .filter(Boolean)
        : [],
    replace,
  };
}

function RestorePanel() {
  const t = useCopy();
  const { fail, info } = useNotices();
  const [file, setFile] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [opened, setOpened] = useState<Opened | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);

  const [mode, setMode] = useState<RestoreMode>("everything");
  const [accounts, setAccounts] = useState<Record<string, Row>>({});
  const [projects, setProjects] = useState<Record<string, Row>>({});
  const [maps, setMaps] = useState<Mapping[]>([]);
  const [select, setSelect] = useState("");
  const [replace, setReplace] = useState(false);
  const [plan, setPlan] = useState<{ plan: Plan; key: string } | null>(null);
  const [applied, setApplied] = useState<Applied | null>(null);
  const [confirming, setConfirming] = useState(false);

  const choice = useMemo(
    () => restoreChoice(mode, accounts, projects, maps, select, replace),
    [mode, accounts, projects, maps, select, replace],
  );
  const key = JSON.stringify(choice);
  // Restore applies exactly what was previewed: any change asks for a new preview.
  const previewed = plan != null && plan.key === key;

  const open = async () => {
    setBusy(true);
    setError(null);
    setApplied(null);
    setPlan(null);
    try {
      const o = await api.backupOpen(file, passphrase);
      setOpened(o);
      setAccounts(Object.fromEntries(o.accounts.map((a) => [a.label, { on: true, to: a.label }])));
      setProjects(Object.fromEntries(o.projects.map((p) => [p.source_root, { on: true, to: "" }])));
      setMaps([{ from: o.suggested_from, to: "" }]);
      setMode(o.accounts.every((a) => a.projects_only) ? "projects" : "everything");
    } catch (e) {
      setError(e);
    } finally {
      // Used once: the backend keeps the opened archive, not the passphrase.
      setPassphrase("");
      setBusy(false);
    }
  };

  const close = async () => {
    await api.backupClose().catch(() => {});
    setOpened(null);
    setPlan(null);
    setApplied(null);
  };

  const preview = async () => {
    setBusy(true);
    setApplied(null);
    try {
      setPlan({ plan: await api.backupPreview(choice), key });
    } catch (e) {
      setPlan(null);
      fail(e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="project-picker backup-panel">
      <div className="panel-heading">
        <div className="summary-icon">
          <Icon name="refresh" />
        </div>
        <div>
          <h2>{t.backupRestoreTitle}</h2>
          <p>{t.backupRestoreLead}</p>
        </div>
      </div>

      {!opened ? (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void open();
          }}
        >
          <label className="sr-only" htmlFor="backup-file">
            {t.backupFileLabel}
          </label>
          <input
            id="backup-file"
            value={file}
            onChange={(e) => setFile(e.target.value)}
            placeholder={t.backupOpenPlaceholder}
            required
          />
          <button
            type="button"
            disabled={busy}
            onClick={async () => {
              const path = await api.chooseBackupFile();
              if (path) setFile(path);
            }}
          >
            {t.browse}
          </button>
          <label className="sr-only" htmlFor="backup-passphrase">
            {t.backupPassphrase}
          </label>
          <input
            id="backup-passphrase"
            type="password"
            autoComplete="current-password"
            value={passphrase}
            onChange={(e) => setPassphrase(e.target.value)}
            placeholder={t.backupPassphrase}
            required
          />
          <button type="submit" className="primary" disabled={busy}>
            {busy ? t.backupOpening : t.backupOpenAction}
          </button>
        </form>
      ) : (
        <>
          <OpenedSummary opened={opened} onClose={close} />

          <fieldset>
            <legend>{t.backupRestoreModeLegend}</legend>
            <div className="choice-list">
              {RESTORE_MODES.map((m) => (
                <label key={m} className="check">
                  <input
                    type="radio"
                    name="restore-mode"
                    checked={mode === m}
                    onChange={() => setMode(m)}
                  />
                  {t.backupRestoreModes[m]}
                </label>
              ))}
            </div>
            <p className="hint">{t.backupRestoreModeHints[mode]}</p>
          </fieldset>

          {mode !== "accounts" && mode !== "reuse" && (
            <fieldset>
              <legend>{t.backupMapsLegend}</legend>
              <p className="hint">{t.backupMapsHint}</p>
              {maps.map((m, i) => (
                <div className="map-row" key={i}>
                  <input
                    aria-label={t.backupMapFrom}
                    value={m.from}
                    placeholder={t.backupMapFrom}
                    onChange={(e) =>
                      setMaps((ms) => ms.map((x, j) => (j === i ? { ...x, from: e.target.value } : x)))
                    }
                  />
                  <span aria-hidden="true">→</span>
                  <input
                    aria-label={t.backupMapTo}
                    value={m.to}
                    placeholder={t.backupMapToPlaceholder}
                    onChange={(e) =>
                      setMaps((ms) => ms.map((x, j) => (j === i ? { ...x, to: e.target.value } : x)))
                    }
                  />
                  <button
                    type="button"
                    onClick={async () => {
                      const path = await api.chooseDirectory();
                      if (path)
                        setMaps((ms) => ms.map((x, j) => (j === i ? { ...x, to: path } : x)));
                    }}
                  >
                    {t.browse}
                  </button>
                  <button
                    type="button"
                    className="quiet"
                    aria-label={t.backupRemoveMapAria}
                    onClick={() => setMaps((ms) => ms.filter((_, j) => j !== i))}
                  >
                    ×
                  </button>
                </div>
              ))}
              <button
                type="button"
                className="quiet"
                onClick={() => setMaps((ms) => [...ms, { from: "", to: "" }])}
              >
                {t.backupAddMap}
              </button>
            </fieldset>
          )}

          {(mode === "everything" || mode === "accounts") && opened.accounts.length > 0 && (
            <fieldset>
              <legend>{t.backupRestoreAccountsLegend}</legend>
              <p className="hint">{t.backupRestoreAccountsHint}</p>
              {opened.accounts.map((a) => (
                <div className="map-row" key={a.label}>
                  <label className="check">
                    <input
                      type="checkbox"
                      checked={accounts[a.label]?.on ?? false}
                      onChange={(e) =>
                        setAccounts((r) => ({ ...r, [a.label]: { ...r[a.label], on: e.target.checked } }))
                      }
                    />
                    {a.label}
                  </label>
                  <span aria-hidden="true">→</span>
                  <input
                    aria-label={t.backupRestoreAs(a.label)}
                    value={accounts[a.label]?.to ?? ""}
                    disabled={!accounts[a.label]?.on}
                    onChange={(e) =>
                      setAccounts((r) => ({ ...r, [a.label]: { ...r[a.label], to: e.target.value } }))
                    }
                  />
                </div>
              ))}
            </fieldset>
          )}

          {mode !== "accounts" && opened.projects.length > 0 && (
            <fieldset>
              <legend>{t.backupRestoreProjectsLegend}</legend>
              <p className="hint">
                {mode === "reuse" ? t.backupReuseDestinationHint : t.backupDestinationHint}
              </p>
              {opened.projects.map((p) => (
                <div className="map-row" key={p.source_root}>
                  <label className="check path">
                    <input
                      type="checkbox"
                      checked={projects[p.source_root]?.on ?? false}
                      onChange={(e) =>
                        setProjects((r) => ({
                          ...r,
                          [p.source_root]: { ...r[p.source_root], on: e.target.checked },
                        }))
                      }
                    />
                    {p.source_root}
                  </label>
                  <span aria-hidden="true">→</span>
                  <input
                    aria-label={t.backupDestinationFor(p.source_root)}
                    value={projects[p.source_root]?.to ?? ""}
                    placeholder={
                      mode === "reuse" ? t.backupReuseDestinationPlaceholder : t.backupFromMapping
                    }
                    disabled={!projects[p.source_root]?.on}
                    onChange={(e) =>
                      setProjects((r) => ({
                        ...r,
                        [p.source_root]: { ...r[p.source_root], to: e.target.value },
                      }))
                    }
                  />
                  <button
                    type="button"
                    disabled={!projects[p.source_root]?.on}
                    onClick={async () => {
                      const path = await api.chooseDirectory();
                      if (path)
                        setProjects((r) => ({
                          ...r,
                          [p.source_root]: { ...r[p.source_root], to: path },
                        }));
                    }}
                  >
                    {t.browse}
                  </button>
                </div>
              ))}
            </fieldset>
          )}

          {mode === "reuse" && (
            <fieldset>
              <legend>{t.backupSelectLegend}</legend>
              <p className="hint">{t.backupSelectHint}</p>
              <textarea
                aria-label={t.backupSelectLegend}
                rows={3}
                value={select}
                placeholder={t.backupSelectPlaceholder}
                onChange={(e) => setSelect(e.target.value)}
              />
            </fieldset>
          )}

          <label className="check">
            <input
              type="checkbox"
              checked={replace}
              onChange={(e) => setReplace(e.target.checked)}
            />
            {t.backupReplace}
          </label>

          <div className="binding-actions">
            <button type="button" disabled={busy} onClick={preview}>
              {busy ? t.working : t.backupPreview}
            </button>
            <button
              type="button"
              className="primary"
              disabled={busy || !previewed}
              title={previewed ? undefined : t.backupPreviewFirst}
              onClick={() => setConfirming(true)}
            >
              {t.backupRestoreAction}
            </button>
          </div>

          {plan && <PlanSummary plan={plan.plan} stale={!previewed} />}
          {applied && <AppliedSummary applied={applied} />}

          {confirming && plan && (
            <Dialog
              title={t.backupRestoreConfirmTitle}
              confirmLabel={t.backupRestoreConfirm}
              onConfirm={async () => {
                const done = await api.backupRestore(choice);
                setApplied(done);
                setPlan(null);
                info(t.backupRestoredNotice(done.written, done.replaced));
              }}
              onClose={() => setConfirming(false)}
            >
              <p>
                {t.backupRestoreConfirmBody(
                  plan.plan.counts.create,
                  plan.plan.replace ? plan.plan.counts.conflict : 0,
                  plan.plan.replace ? 0 : plan.plan.counts.conflict,
                )}
              </p>
            </Dialog>
          )}
        </>
      )}
      {error != null && <Failure error={error} retry={() => void open()} />}
    </section>
  );
}

function OpenedSummary({ opened: o, onClose }: { opened: Opened; onClose: () => void }) {
  const t = useCopy();
  return (
    <div className="effective">
      <span className="eyebrow">{t.backupVerified(o.files)}</span>
      <h3 className="path">{o.file}</h3>
      <p>
        {t.backupOrigin(
          new Date(o.created_at * 1000).toLocaleString(t.locale),
          o.source_platform,
          o.source_home,
          o.tool_version,
        )}
      </p>
      <ul className="file-list">
        {o.accounts.map((a) => (
          <li key={a.label}>
            <span className="badge">{a.label}</span>
            {t.backupOpenedAccount(a.files, a.projects_only)}
          </li>
        ))}
        {o.projects.map((p) => (
          <li key={p.source_root}>
            <span className="badge">{t.backupProjectBadge}</span>
            <span className="path">{p.source_root}</span> · {t.backupFileCount(p.files)}
          </li>
        ))}
        {o.bindings.map((b) => (
          <li key={b.dir}>
            <span className="badge">{t.backupBindingBadge}</span>
            <span className="path">{b.dir}</span> → {b.account}
          </li>
        ))}
        {o.default_account && (
          <li>
            <span className="badge">{t.defaultBadge}</span>
            {o.default_account}
          </li>
        )}
      </ul>
      {o.redactions.length > 0 && (
        <details>
          <summary>{t.backupRedacted(o.redactions.length)}</summary>
          <ul className="file-list">
            {o.redactions.map((r) => (
              <li key={`${r.path}:${r.field}`}>
                <span className="path">{r.path}</span> <code>{r.field}</code>
              </li>
            ))}
          </ul>
        </details>
      )}
      <LeftOut excluded={o.excluded} />
      <div className="binding-actions">
        <button type="button" className="quiet" onClick={onClose}>
          {t.backupCloseAction}
        </button>
      </div>
    </div>
  );
}

const STEP_MARK: Record<Step, string> = {
  create: "+",
  same: "=",
  conflict: "!",
  incompatible: "×",
};

function PlanSummary({ plan, stale }: { plan: Plan; stale: boolean }) {
  const t = useCopy();
  const [filter, setFilter] = useState<Step | "all">(
    plan.counts.conflict ? "conflict" : plan.counts.incompatible ? "incompatible" : "create",
  );
  const [shown, setShown] = useState(FILE_PAGE);
  const files = plan.files.filter((f) => filter === "all" || f.step === filter);
  const logins = plan.notes.flatMap((n) => (n.kind === "login_needed" ? [n.account] : []));
  return (
    <div className="effective" aria-live="polite">
      <span className="eyebrow">{stale ? t.backupPlanStale : t.backupPlanTitle}</span>
      <h3>
        {t.backupPlanCounts(
          plan.counts.create,
          plan.counts.same,
          plan.counts.conflict,
          plan.counts.incompatible,
        )}
      </h3>
      <ul className="file-list">
        {plan.accounts.map((a) => (
          <li key={a.from}>
            <span className="badge">{a.from}</span>
            {t.backupAccountMove(a.to, a.new, a.whole)}
          </li>
        ))}
        {plan.projects.map((p) => (
          <li key={p.from}>
            <span className="badge">{t.backupProjectBadge}</span>
            <span className="path">{p.from}</span> → <span className="path">{p.to}</span>
          </li>
        ))}
        {plan.left_out.map((p) => (
          <li key={p} className="warning">
            {t.backupProjectLeftOut(p)}
          </li>
        ))}
        {plan.links.map((l) => (
          <li key={l.dir}>
            <span className="badge">{t.backupBindingBadge}</span>
            <span className="path">{l.dir}</span> → {l.account} · {t.backupSteps[l.step]}
          </li>
        ))}
        {plan.default_account && (
          <li>
            <span className="badge">{t.defaultBadge}</span>
            {plan.default_account.account} · {t.backupSteps[plan.default_account.step]}
          </li>
        )}
      </ul>

      {plan.notes.length > 0 && (
        <ul className="file-list notes">
          {plan.notes.map((n, i) =>
            n.kind === "login_needed" ? null : <li key={i}>{noteText(t, n)}</li>,
          )}
          {logins.length > 0 && <li className="warning">{t.backupLoginNeeded(list(t, logins))}</li>}
        </ul>
      )}

      <div className="step-filter" role="group" aria-label={t.backupFilterAria}>
        {(["all", "create", "conflict", "incompatible", "same"] as const).map((s) => (
          <button
            key={s}
            type="button"
            className={filter === s ? "active" : "quiet"}
            aria-pressed={filter === s}
            onClick={() => {
              setFilter(s);
              setShown(FILE_PAGE);
            }}
          >
            {s === "all" ? t.backupAllFiles(plan.files.length) : t.backupStepFilter(s, countOf(plan, s))}
          </button>
        ))}
      </div>
      <ul className="file-list files">
        {files.slice(0, shown).map((f) => (
          <FileRow key={f.archive} file={f} />
        ))}
      </ul>
      {shown < files.length && (
        <button className="quiet" onClick={() => setShown((n) => n + FILE_PAGE)}>
          {t.showMore(files.length - shown)}
        </button>
      )}
      <p className="hint">{plan.replace ? t.backupReplaceOn : t.backupReplaceOff}</p>
    </div>
  );
}

function countOf(plan: Plan, s: Step): number {
  return plan.counts[s];
}

function FileRow({ file: f }: { file: FileStep }) {
  const t = useCopy();
  return (
    <li className={`step-${f.step}`}>
      <span className="mark" aria-label={t.backupSteps[f.step]}>
        {STEP_MARK[f.step]}
      </span>
      <span className="path">{f.dest}</span>
      {f.why && (
        <span className="why">
          {f.why === "same_name_other_case" && f.other
            ? t.backupWhyCase(f.other)
            : t.backupWhy[f.why]}
        </span>
      )}
    </li>
  );
}

function noteText(t: ReturnType<typeof useCopy>, n: Plan["notes"][number]): string {
  switch (n.kind) {
    case "project_dir_renamed":
      return t.backupNoteRenamed(n.account, n.from, n.to);
    case "project_dir_unmapped":
      return t.backupNoteUnmapped(n.account, n.name);
    case "claude_json_paths_mapped":
      return t.backupNoteJsonMapped(n.account, n.keys);
    case "needs_review":
      return t.backupNoteReview(n.path, n.field);
    case "link_unmapped":
      return t.backupNoteLinkUnmapped(n.dir);
    case "reinstall":
      return t.backupNoteReinstall(n.path);
    case "login_needed":
      return t.backupLoginNeeded(n.account);
  }
}

function AppliedSummary({ applied: a }: { applied: Applied }) {
  const t = useCopy();
  return (
    <div className="effective" role="status">
      <span className="eyebrow">{t.backupRestored}</span>
      <h3>{t.backupAppliedCounts(a.written, a.replaced, a.skipped, a.switch_changes)}</h3>
      {a.originals && <p className="path">{t.backupOriginalsKept(a.originals)}</p>}
      <p>{t.backupResumeNote}</p>
    </div>
  );
}
