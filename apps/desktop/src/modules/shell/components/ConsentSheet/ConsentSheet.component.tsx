import { HoldToConfirm, Key, Label, Notice, Sheet } from "@flexdeck/ui";

import { permissionLabel } from "../../lib/plugins.lib.ts";
import type { Consent } from "../../lib/types.ts";
import { PluginFace } from "../PluginFace/PluginFace.component.tsx";

/**
 * Nothing installs without the person seeing what it is and what it may do.
 * The agent enforces that too — an install without the accepted permissions is
 * refused — so this sheet is the explanation, not the lock. Installing is a hold,
 * not a tap: plugins run as programs on this computer.
 */
export function ConsentSheet({
  consent,
  busy,
  error = null,
  onAccept,
  onCancel,
}: {
  consent: Consent | null;
  busy: boolean;
  error?: string | null;
  onAccept: () => void;
  onCancel: () => void;
}) {
  return (
    <Sheet open={consent !== null} onClose={onCancel} label="Install a plugin">
      {consent ? (
        <div className="flex flex-col gap-4">
          <div className="flex items-center gap-3.5">
            <PluginFace name={consent.package.name} icon={consent.icon ?? null} size="lg" />
            <div className="min-w-0">
              <Label>{consent.repo ? `Install · ${consent.repo.name}` : "Install from a file"}</Label>
              <h3 className="m-0 mt-1 truncate text-xl font-semibold">
                {consent.package.name} <span className="font-mono text-xs text-faint">{consent.package.version}</span>
              </h3>
              {consent.package.author ? <p className="m-0 text-[12px] text-muted">by {consent.package.author}</p> : null}
            </div>
          </div>
          {consent.package.description ? <p className="m-0 text-[13px] leading-relaxed text-ink-2">{consent.package.description}</p> : null}
          {consent.replaces ? <p className="m-0 text-[12px] text-muted">Replaces version {consent.replaces}. Its settings are kept.</p> : null}

          <div className="flex flex-col gap-2">
            <Label>{consent.permissions.length > 0 ? "It will be able to" : "It asks for no permissions"}</Label>
            {consent.permissions.map((permission) => (
              <div key={permission} className="flex items-start gap-2.5 rounded-lg border border-line bg-body px-3 py-2 text-[13px]">
                <span className="mt-0.5 font-mono text-[10px] tracking-[0.12em] text-signal uppercase">{permission}</span>
                <span className="text-ink-2">{permissionLabel(permission)}</span>
              </div>
            ))}
          </div>

          {consent.repo?.kind === "custom" ? (
            <Notice tone="warn">
              From <strong>{consent.repo.name}</strong>, a repository you added. FlexDeck has not reviewed it; install only if you trust whoever runs it.
            </Notice>
          ) : null}

          {error ? <Notice>{error}</Notice> : null}

          <div className="flex gap-2.5">
            <Key size="lg" className="flex-1" onClick={onCancel} disabled={busy}>
              Cancel
            </Key>
            <HoldToConfirm tone="signal" size="lg" className="flex-[2]" disabled={busy} onConfirm={onAccept}>
              Hold to install
            </HoldToConfirm>
          </div>
        </div>
      ) : null}
    </Sheet>
  );
}
