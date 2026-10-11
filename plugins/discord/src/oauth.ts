/**
 * Turning the code Discord's consent dialog hands back into tokens, and
 * keeping them fresh.
 *
 * Two ways, depending on whose Discord application this is:
 *
 * * **Your own application** ("bring your own"): you created it, so its secret
 *   is yours, it lives only in this plugin's settings on your computer, and the
 *   exchange uses it.
 * * **A public application** with no secret anywhere: the exchange proves it
 *   started the flow with PKCE — a random verifier whose hash went into the
 *   AUTHORIZE request. No secret is ever shipped inside this plugin.
 */

import { createHash, randomBytes } from "node:crypto";

export interface Tokens {
  accessToken: string;
  refreshToken: string;
  /** Milliseconds since the epoch. */
  expiresAt: number;
}

export interface Credentials {
  clientId: string;
  /** Empty for a public application. */
  clientSecret: string;
  /** Discord's API, overridable so the tests can run against a fake. */
  apiBase: string;
  /** Must match one registered on the application. */
  redirectUri: string;
}

/** A PKCE verifier and the challenge sent with AUTHORIZE. */
export function pkce(): { verifier: string; challenge: string } {
  const verifier = randomBytes(48).toString("base64url");
  const challenge = createHash("sha256").update(verifier).digest("base64url");
  return { verifier, challenge };
}

export class OAuthError extends Error {}

async function token(credentials: Credentials, fields: Record<string, string>): Promise<Tokens> {
  const body = new URLSearchParams({ client_id: credentials.clientId, ...fields });
  if (credentials.clientSecret) body.set("client_secret", credentials.clientSecret);
  let response: Response;
  try {
    response = await fetch(`${credentials.apiBase.replace(/\/+$/, "")}/oauth2/token`, {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body,
    });
  } catch (error) {
    throw new OAuthError(`could not reach Discord to sign in: ${String(error)}`);
  }
  const answer = (await response.json().catch(() => ({}))) as Record<string, unknown>;
  if (!response.ok) {
    const why = String(answer.error_description ?? answer.error ?? response.statusText);
    throw new OAuthError(`Discord refused the sign-in: ${why}`);
  }
  return {
    accessToken: String(answer.access_token ?? ""),
    refreshToken: String(answer.refresh_token ?? ""),
    expiresAt: Date.now() + Number(answer.expires_in ?? 0) * 1000,
  };
}

/** Exchange AUTHORIZE's code for tokens. `verifier` is the PKCE one, if used. */
export function exchange(credentials: Credentials, code: string, verifier?: string): Promise<Tokens> {
  const fields: Record<string, string> = { grant_type: "authorization_code", code };
  if (credentials.redirectUri) fields.redirect_uri = credentials.redirectUri;
  if (verifier) fields.code_verifier = verifier;
  return token(credentials, fields);
}

/** A fresh access token from a refresh token. Discord rotates the refresh token too. */
export function refresh(credentials: Credentials, refreshToken: string): Promise<Tokens> {
  return token(credentials, { grant_type: "refresh_token", refresh_token: refreshToken });
}
