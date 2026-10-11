import { useState } from "react";

import { cn } from "@flexdeck/ui";

import { Button } from "../../../../common/components/Button/Button.component.tsx";
import { describeError } from "../../../../services/api/errors.ts";

export interface AddDesktopProps {
  /** Pair with the desktop showing a code. Resolves to its name and must throw on failure. */
  pair: (code: string) => Promise<string>;
  /** Centre the text and the form, for a page that is centred around it. */
  centered?: boolean;
}

/**
 * Connecting this browser to a desktop on another computer, by the code it
 * shows under **Browsers** in the desktop app.
 *
 * There is no account and no sign-in: the code is the whole ceremony. Whoever
 * can read it off the desktop's screen is trusted, and this browser is
 * remembered by that desktop until it is removed there.
 *
 * A bare form rather than a card, so the caller decides the surrounding surface.
 */
export function AddDesktop({ pair, centered = false }: AddDesktopProps) {
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  return (
    <div className={cn("flex flex-col gap-3", centered && "items-center text-center")}>
      <p className="m-0 text-[13px] leading-relaxed text-muted">
        On the computer you want to control, open the FlexDeck app, switch on web access, and under{" "}
        <strong className="text-ink">Pair → Browser</strong> show a code. Type it here. It is not the tablet code —
        that one pairs a tablet over the local network.
      </p>
      <form
        className={cn("flex flex-wrap gap-2", centered && "justify-center")}
        onSubmit={async (event) => {
          event.preventDefault();
          const trimmed = code.trim();
          if (!trimmed || busy) {
            return;
          }

          setBusy(true);
          try {
            const name = await pair(trimmed);
            setNotice(`Connected to ${name}.`);
            setError(null);
            setCode("");
          } catch (cause) {
            setError(describeError(cause));
          } finally {
            setBusy(false);
          }
        }}
      >
        <input
          value={code}
          onChange={(event) => setCode(event.target.value.toUpperCase())}
          placeholder="XXXX-XXXX"
          autoComplete="off"
          spellCheck={false}
          maxLength={11}
          aria-label="Code shown by the desktop app"
          className="h-10 w-48 rounded-[10px] border border-line bg-well px-3 font-mono text-sm tracking-[0.2em] uppercase shadow-well outline-none placeholder:text-faint focus:outline-2 focus:outline-signal"
        />
        <Button type="submit" disabled={busy}>
          {busy ? "Connecting…" : "Connect"}
        </Button>
      </form>

      {notice ? <p className="m-0 text-sm text-ok">{notice}</p> : null}
      {error ? <p className="m-0 text-sm text-bad">{error}</p> : null}
    </div>
  );
}
