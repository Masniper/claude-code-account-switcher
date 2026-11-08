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
