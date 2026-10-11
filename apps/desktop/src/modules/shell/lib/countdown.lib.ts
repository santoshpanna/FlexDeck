export interface Remaining {
  expired: boolean;
  seconds: number;
  /** `m:ss`, the shape a countdown is read in. */
  text: string;
}

/**
 * How long a pairing PIN has left.
 *
 * Takes `now` rather than reading the clock, so the boundary is testable without
 * waiting for it. Expiry is `<= 0`, matching the agent, which treats the instant
 * itself as already closed.
 */
export function remaining(expiresAt: number, now: number): Remaining {
  const seconds = Math.max(0, Math.floor(expiresAt - now));
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;

  return {
    expired: seconds <= 0,
    seconds,
    text: `${minutes}:${rest.toString().padStart(2, "0")}`,
  };
}
