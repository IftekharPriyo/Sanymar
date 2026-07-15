import { useCallback, useEffect, useRef, useState } from "react";
import { sanymarService } from "../services/sanymar";
import type { Dashboard } from "../types/domain";

export function dashboardErrorMessage(
  reason: unknown,
  fallback: string,
): string {
  if (reason instanceof Error) return reason.message;
  if (
    typeof reason === "object" &&
    reason !== null &&
    "message" in reason &&
    typeof reason.message === "string"
  )
    return reason.message;
  return fallback;
}

export function useDashboard() {
  const [dashboard, setDashboard] = useState<Dashboard | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const refreshInFlight = useRef(false);
  const hasDashboard = useRef(false);
  const consecutiveRefreshFailures = useRef(0);
  const liveMode = dashboard?.mockMode === false;

  const refresh = useCallback(async () => {
    if (refreshInFlight.current) return;
    refreshInFlight.current = true;
    try {
      setDashboard(await sanymarService.getDashboard());
      hasDashboard.current = true;
      consecutiveRefreshFailures.current = 0;
      setError(null);
    } catch (reason) {
      consecutiveRefreshFailures.current += 1;
      if (!hasDashboard.current || consecutiveRefreshFailures.current >= 2) {
        setError(
          dashboardErrorMessage(reason, "Unable to load Sanymar state."),
        );
      }
    } finally {
      refreshInFlight.current = false;
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (!liveMode) return;
    const interval = window.setInterval(() => void refresh(), 5_000);
    const refreshWhenFocused = () => void refresh();
    window.addEventListener("focus", refreshWhenFocused);
    return () => {
      window.clearInterval(interval);
      window.removeEventListener("focus", refreshWhenFocused);
    };
  }, [liveMode, refresh]);

  const generate = async () => {
    setBusy(true);
    setNotice(null);
    try {
      const result = await sanymarService.generateTestSegment();
      setDashboard((current) =>
        current
          ? {
              ...current,
              recentScript: result.dialogue,
              broadcastState: result.broadcastState,
            }
          : current,
      );
      setError(null);
    } catch (reason) {
      setError(dashboardErrorMessage(reason, "Mock generation failed."));
    } finally {
      setBusy(false);
    }
  };

  const speak = async () => {
    setBusy(true);
    try {
      const result = await sanymarService.speakTestSegment();
      setNotice(result.message);
      setError(null);
    } catch (reason) {
      setError(dashboardErrorMessage(reason, "Mock speech failed."));
    } finally {
      setBusy(false);
    }
  };

  return { dashboard, busy, notice, error, generate, speak };
}
