import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { Dialog, Failure } from "../components";
import type { WatchStatus } from "../contract";
import { useCopy } from "../i18n";

const POLL_MS = 1000;

export function WatchPage() {
  const t = useCopy();
  const [status, setStatus] = useState<WatchStatus | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [confirming, setConfirming] = useState(false);
  const load = useCallback(() => {
    api.watchStatus().then((next) => {
      setStatus(next);
      setError(null);
    }, setError);
  }, []);
  useEffect(() => {
    load();
    const timer = window.setInterval(load, POLL_MS);
    return () => window.clearInterval(timer);
  }, [load]);

  if (error != null) return <Failure error={error} retry={load} />;
  if (status == null) return <div className="empty" role="status">{t.watchReading}</div>;
  if (!status.supported) return <div className="banner"><span>{t.watchUnsupported}</span></div>;

  return (
    <>
      <div className="banner">
        <span>{t.watchBanner}</span>
      </div>
      <div className="setting-row">
        <div>
          <strong>{t.watchTitle}</strong>
          <span>{status.enabled ? t.watchOn : t.watchOff}</span>
        </div>
        <button
          onClick={() => {
            if (status.enabled) api.setWatch(false).then(setStatus, setError);
            else setConfirming(true);
          }}
        >
          {status.enabled ? t.watchTurnOff : t.watchTurnOn}
        </button>
      </div>
      <div className="setting-row">
        <div>
          <strong>{t.watchVpn}</strong>
          <span>{status.vpn ?? t.watchNoVpn}</span>
        </div>
      </div>
      <div className="setting-row">
        <div>
          <strong>{t.watchFrozen}</strong>
          <span>{status.frozen > 0 ? t.watchFrozenCount(status.frozen) : t.watchNothingFrozen}</span>
        </div>
      </div>
      <VpnZone connected={status.vpn != null} />
      <p className="footnote">{t.watchLimits}</p>
      {confirming && (
        <Dialog
          title={t.watchOnTitle}
          confirmLabel={t.watchTurnOn}
          onConfirm={async () => setStatus(await api.setWatch(true))}
          onClose={() => setConfirming(false)}
        >
          <p>{status.vpn ? t.watchOnBody(status.vpn) : t.watchOnBodyNoVpn}</p>
        </Dialog>
      )}
    </>
  );
}

function VpnZone({ connected }: { connected: boolean }) {
  const t = useCopy();
  const [location, setLocation] = useState<Awaited<ReturnType<typeof api.vpnExit>> | null>(null);
  const [restore, setRestore] = useState(false);
  const [pending, setPending] = useState(false);
  const [notice, setNotice] = useState("");
  useEffect(() => {
    api.canRestoreZone().then(setRestore, () => setNotice(t.watchZoneFailed));
  }, [t]);
  const run = async (action: () => Promise<void>) => {
    setPending(true);
    setNotice("");
    try {
      await action();
    } catch {
      setNotice(t.watchZoneFailed);
    } finally {
      setPending(false);
    }
  };

  return (
    <>
      <div className="setting-row">
        <div>
          <strong>{t.watchZoneTitle}</strong>
          <span>{t.watchZoneHelp}</span>
        </div>
        <button disabled={!connected || pending} onClick={() => void run(async () => setLocation(await api.vpnExit()))}>
          {pending ? t.working : t.watchZoneSync}
        </button>
        {restore && (
          <button disabled={pending} onClick={() => void run(async () => {
            setRestore(await api.changeVpnZone(null));
            setNotice(t.watchZoneRestored);
          })}>{t.watchZoneRestore}</button>
        )}
      </div>
      {notice && <p className="footnote" role="status">{notice}</p>}
      {location && (
        <Dialog
          title={t.watchZoneTitle}
          confirmLabel={t.watchZoneApply}
          onClose={() => setLocation(null)}
          onConfirm={async () => {
            try {
              setRestore(await api.changeVpnZone(location.zone));
              setNotice(t.watchZoneApplied(location.zone));
            } catch (error) {
              setRestore(await api.canRestoreZone());
              throw error;
            }
          }}
        >
          <p>{t.watchZonePreview([location.city, location.country].filter(Boolean).join(", "), location.zone)}</p>
        </Dialog>
      )}
    </>
  );
}
