import { useEffect, useId, useState } from "react";

import type { ActionDefinition, ArgumentChoice } from "../../../../services/api/api.ts";
import type { LoadChoices } from "../../hooks/useArgumentChoices/useArgumentChoices.hook.ts";
import type { Step } from "../../lib/slider.lib.ts";
import { control } from "../Flow/fields.tsx";

export interface StepPickerProps {
  /** A label for the picker, read by screen readers and shown above it. */
  label: string;
  /** The actions on offer. */
  actions: readonly ActionDefinition[];
  value: Step | null;
  /** Called with the chosen step, or `null` for "nothing". Committed on blur. */
  onChange: (step: Step | null) => void;
  /** An argument filled in some other way (a slider's value), so not shown. */
  hidden?: string;
  loadChoices?: LoadChoices;
}

const keyOf = (step: { plugin: string; action: string }) => `${step.plugin}:${step.action}`;

/** The value of an argument picker's "type something else" entry. */
const OTHER = "\u0000other";

/**
 * One action and its arguments, compactly: what a slider drives, what a hold
 * does when pressed or let go. The full action form is for a button's tap;
 * this is the same idea in the space a sidebar sub-section has.
 *
 * Arguments are committed when a field loses focus, so typing does not spend a
 * revision per keystroke; choosing an action commits at once.
 */
export function StepPicker({ label, actions, value, onChange, hidden, loadChoices }: StepPickerProps) {
  const id = useId();
  const [args, setArgs] = useState<Record<string, string>>(value?.args ?? {});
  const [choices, setChoices] = useState<Record<string, ArgumentChoice[]>>({});
  // Arguments the person chose to type rather than pick from the list.
  const [typing, setTyping] = useState<Record<string, boolean>>({});
  const chosen = value ? actions.find((action) => keyOf(action) === keyOf(value)) : undefined;

  // Follow the selection, and the same step changed elsewhere.
  const valueKey = value ? `${keyOf(value)}|${JSON.stringify(value.args)}` : "";
  useEffect(() => {
    setArgs(value?.args ?? {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [valueKey]);
  useEffect(() => setTyping({}), [value?.plugin, value?.action]);

  useEffect(() => {
    if (!value || !loadChoices) {
      setChoices({});
      return;
    }
    let current = true;
    loadChoices(value.plugin, value.action, value.args)
      .then((answer) => current && setChoices(answer.arguments))
      .catch(() => current && setChoices({}));
    return () => {
      current = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [valueKey, loadChoices]);

  const commit = (next: Record<string, string>) => {
    if (value) onChange({ ...value, args: next });
  };

  return (
    <div className="flex flex-col gap-2.5">
      <div className="flex flex-col gap-1.5">
        <label className="text-[11px] text-muted" htmlFor={`${id}-action`}>
          {label}
        </label>
        <select
          className={control}
          id={`${id}-action`}
          value={value ? keyOf(value) : ""}
          onChange={(event) => {
            const picked = actions.find((action) => keyOf(action) === event.currentTarget.value);
            onChange(picked ? { plugin: picked.plugin, action: picked.action, args: {} } : null);
          }}
        >
          <option value="">(nothing)</option>
          {actions.map((action) => (
            <option key={keyOf(action)} value={keyOf(action)}>
              {action.pluginName ?? action.plugin} — {action.description || action.action}
            </option>
          ))}
          {value && !chosen ? <option value={keyOf(value)}>{keyOf(value)} (not installed)</option> : null}
        </select>
      </div>
      {(chosen?.arguments ?? [])
        .filter((argument) => argument.name !== hidden)
        .map((argument) => {
          const offered = choices[argument.name] ?? [];
          const current = args[argument.name] ?? "";
          // A list when the plugin offered one and the value is on it (or
          // blank); a text box for anything else, or when asked for.
          const listed =
            offered.length > 0 &&
            !typing[argument.name] &&
            (current === "" || offered.some((choice) => choice.value === current));
          return (
            <div className="flex flex-col gap-1.5" key={argument.name}>
              <label className="text-[11px] text-muted" htmlFor={`${id}-${argument.name}`}>
                {argument.description || argument.name}
                {argument.optional ? " (optional)" : ""}
              </label>
              {argument.kind === "choice" ? (
                <select
          className={control}
                  id={`${id}-${argument.name}`}
                  value={args[argument.name] ?? argument.example}
                  onChange={(event) => {
                    const next = { ...args, [argument.name]: event.currentTarget.value };
                    setArgs(next);
                    commit(next);
                  }}
                >
                  {(argument.choices ?? []).map((choice) => (
                    <option key={choice} value={choice}>
                      {choice}
                    </option>
                  ))}
                </select>
              ) : listed ? (
                <select
          className={control}
                  id={`${id}-${argument.name}`}
                  value={current}
                  onChange={(event) => {
                    const picked = event.currentTarget.value;
                    if (picked === OTHER) {
                      setTyping({ ...typing, [argument.name]: true });
                      return;
                    }
                    const next = { ...args, [argument.name]: picked };
                    setArgs(next);
                    commit(next);
                  }}
                >
                  <option value="">{argument.optional ? "(none)" : "Choose…"}</option>
                  {offered.map((choice) => (
                    <option key={choice.value} value={choice.value}>
                      {choice.label ?? choice.value}
                    </option>
                  ))}
                  <option value={OTHER}>Other…</option>
                </select>
              ) : (
                <>
                  <input
                    className={control}
                    id={`${id}-${argument.name}`}
                    type="text"
                    value={current}
                    placeholder={argument.example}
                    onChange={(event) => setArgs({ ...args, [argument.name]: event.currentTarget.value })}
                    onBlur={() => commit(args)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") commit(args);
                    }}
                  />
                </>
              )}
            </div>
          );
        })}
    </div>
  );
}
