import { useEffect, useState } from "react";

/**
 * The current time in Unix seconds, refreshed so a countdown can tick.
 *
 * Reads `Date.now()` rather than being passed a time, because the only consumers
 * are countdowns, and a countdown that does not advance is not a countdown.
 */
export function useNow(intervalMs = 1000): number {
  const [now, setNow] = useState(() => Date.now() / 1000);

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now() / 1000), intervalMs);
    return () => clearInterval(timer);
  }, [intervalMs]);

  return now;
}
