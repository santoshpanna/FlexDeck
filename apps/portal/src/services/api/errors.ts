import { ApiError } from "./api.ts";

/**
 * Turn a thrown value into something worth showing a person.
 *
 * The one thing worth special-casing is an expired session: "401 Unauthorized"
 * tells a user nothing they can act on, whereas "session missing or expired"
 * points at the actual remedy. Everything else uses the message the server sent,
 * because that message was written for a human — the portal's own wording would
 * be a guess about a failure it did not diagnose.
 */
export function describeError(cause: unknown): string {
  if (cause instanceof ApiError) {
    return cause.isUnauthenticated ? "This browser is no longer connected to that desktop. Connect it again." : cause.message;
  }
  return cause instanceof Error ? cause.message : String(cause);
}
