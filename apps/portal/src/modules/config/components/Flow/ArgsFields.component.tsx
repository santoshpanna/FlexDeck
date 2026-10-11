import { useEffect, useId, useState } from "react";

import type { ActionDefinition, ArgumentChoice } from "../../../../services/api/api.ts";
import { control, ValueInput } from "./fields.tsx";
import { useFlow } from "./flow.context.ts";

/** The "type something else" entry in a list of a plugin's choices. */
const OTHER = "\u0000other";

/**
 * An action's arguments, built from what the action declares. Each argument
 * gets the input its kind asks for; where the plugin can say what an argument
 * may be right now — OBS's scenes, the devices playing sound — it becomes a
 * list, with typing still possible (OBS may be closed). Values are text, or a
 * live value as `{{name}}`.
 */
export function ArgsFields({
  definition,
  args,
  onChange,
  hidden,
}: {
  definition: ActionDefinition;
  args: Record<string, string>;
  onChange: (args: Record<string, string>) => void;
  /** An argument filled in another way (a slider's value), so not shown. */
  hidden?: string;
}) {
  const { loadChoices, variables } = useFlow();
  const id = useId();
  const [choices, setChoices] = useState<Record<string, ArgumentChoice[]>>({});
  const [failed, setFailed] = useState<string | null>(null);
  const [typing, setTyping] = useState<Record<string, boolean>>({});
  const key = `${definition.plugin}:${definition.action}|${JSON.stringify(args)}`;

  useEffect(() => {
    if (!loadChoices || definition.arguments.length === 0) return;
    let current = true;
    loadChoices(definition.plugin, definition.action, args).then(
      (answer) => {
        if (!current) return;
        setChoices(answer.arguments);
        setFailed(answer.error ?? null);
      },
      (error: unknown) => current && setFailed(error instanceof Error ? error.message : String(error)),
    );
    return () => {
      current = false;
    };
    // `key` stands for the action and its saved arguments.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, loadChoices]);

  const shown = definition.arguments.filter((argument) => argument.name !== hidden);
  if (shown.length === 0) return <p className="m-0 text-[12px] text-faint">Takes no settings.</p>;

  const set = (name: string, value: string) => onChange({ ...args, [name]: value });

  return (
    <div className="@container grid gap-2.5 @md:grid-cols-2">
      {shown.map((argument) => {
        const fieldId = `${id}-${argument.name}`;
        const current = args[argument.name] ?? "";
        const offered = choices[argument.name] ?? [];
        const listed =
          offered.length > 0 && !typing[argument.name] && (current === "" || offered.some((choice) => choice.value === current));
        return (
          <label key={argument.name} htmlFor={fieldId} className="flex min-w-0 flex-col gap-1.5">
            <span className="text-[11px] text-muted">
              {argument.description || argument.name}
              {argument.optional ? <span className="text-faint"> · optional</span> : null}
            </span>
            {argument.kind === "choice" ? (
              <select
                id={fieldId}
                className={control}
                value={current || argument.choices?.[0] || ""}
                onChange={(event) => set(argument.name, event.target.value)}
              >
                {(argument.choices ?? []).map((choice) => (
                  <option key={choice} value={choice}>
                    {choice}
                  </option>
                ))}
              </select>
            ) : argument.kind === "bool" ? (
              <select id={fieldId} className={control} value={current || "true"} onChange={(event) => set(argument.name, event.target.value)}>
                <option value="true">On</option>
                <option value="false">Off</option>
              </select>
            ) : listed ? (
              <select
                id={fieldId}
                className={control}
                value={current}
                onChange={(event) => {
                  if (event.target.value === OTHER) setTyping({ ...typing, [argument.name]: true });
                  else set(argument.name, event.target.value);
                }}
              >
                <option value="">{argument.optional ? "(none)" : "Choose…"}</option>
                {offered.map((choice) => (
                  <option key={choice.value} value={choice.value}>
                    {choice.label ?? choice.value}
                  </option>
                ))}
                <option value={OTHER}>Type a value…</option>
              </select>
            ) : (
              <ValueInput
                label={argument.description || argument.name}
                value={current}
                onCommit={(value) => set(argument.name, value)}
                variables={variables}
                placeholder={argument.example ? `e.g. ${argument.example}` : undefined}
              />
            )}
          </label>
        );
      })}
      {failed ? <p className="m-0 text-[11px] text-warn @md:col-span-2">Couldn’t list the options: {failed}</p> : null}
    </div>
  );
}
