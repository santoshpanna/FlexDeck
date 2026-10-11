import { Segmented } from "@flexdeck/ui";

import type { CompareOp, Condition } from "../../../../services/api/api.ts";
import { COMPARE_OPS, conditionParts, joinConditions, literalValue, valueText } from "../../lib/flow.lib.ts";
import { control, ValueInput, VariableInput } from "./fields.tsx";
import { useFlow } from "./flow.context.ts";

const blankCompare: Condition = { kind: "compare", left: { variable: "" }, op: "eq", right: { bool: true } };

/**
 * A condition as sentences: "obs.recording is true", one per line, joined by
 * ALL or ANY. Deeper nesting (a NOT, a group in a group) is shown as written and
 * left intact — rare enough that a raw line beats a builder nobody can read.
 */
export function ConditionEditor({ condition, onChange }: { condition: Condition; onChange: (next: Condition) => void }) {
  const { variables } = useFlow();
  const { mode, parts } = conditionParts(condition);

  const setPart = (index: number, part: Condition) =>
    onChange(joinConditions(mode, parts.map((current, i) => (i === index ? part : current))));
  const removePart = (index: number) => {
    const rest = parts.filter((_, i) => i !== index);
    onChange(rest.length > 0 ? joinConditions(mode, rest) : blankCompare);
  };

  return (
    <div className="flex flex-col gap-2">
      {parts.length > 1 ? (
        <Segmented
          size="sm"
          label="How the conditions combine"
          value={mode}
          onChange={(next) => onChange(joinConditions(next, parts))}
          options={[
            { value: "all", label: "All of" },
            { value: "any", label: "Any of" },
          ]}
          className="self-start"
        />
      ) : null}
      {parts.map((part, index) =>
        part.kind === "compare" ? (
          <div key={index} className="flex flex-wrap items-center gap-2">
            <VariableInput
              label="Value to check"
              value={"variable" in part.left ? part.left.variable : valueText(part.left)}
              onCommit={(name) => setPart(index, { ...part, left: { variable: name } })}
              variables={variables}
              className="w-auto min-w-[150px] flex-1"
            />
            <select
              aria-label="Comparison"
              className={`${control} w-auto`}
              value={part.op}
              onChange={(event) => setPart(index, { ...part, op: event.target.value as CompareOp })}
            >
              {COMPARE_OPS.map((op) => (
                <option key={op.op} value={op.op}>
                  {op.label}
                </option>
              ))}
            </select>
            <ValueInput
              label="Compared with"
              value={valueText(part.right)}
              onCommit={(text) => setPart(index, { ...part, right: literalValue(text) })}
              variables={variables}
              placeholder="true, 42, Live…"
              className="w-auto min-w-[100px] flex-1"
            />
            {parts.length > 1 ? (
              <button
                type="button"
                aria-label="Remove this condition"
                onClick={() => removePart(index)}
                className="cursor-pointer px-1 text-faint hover:text-bad"
              >
                ×
              </button>
            ) : null}
          </div>
        ) : (
          <code key={index} className="rounded-md bg-well px-2 py-1 font-mono text-[11px] text-muted">
            {JSON.stringify(part)}
          </code>
        ),
      )}
      <button
        type="button"
        onClick={() => onChange(joinConditions(mode, [...parts, blankCompare]))}
        className="lbl cursor-pointer self-start hover:text-ink"
      >
        + Add condition
      </button>
    </div>
  );
}
