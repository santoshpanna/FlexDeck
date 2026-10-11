import type { Block, CompareOp, Condition, Trigger, ValueRef } from "../../../../services/api/api.ts";

/**
 * One line of human-readable text that names what triggers an automation.
 *
 * The list view shows this next to the name, so a screen reader or a glance
 * tells the user "fires when volume changes" without opening the editor.
 */
export function describeTrigger(trigger: Trigger): string {
  switch (trigger.on) {
    case "manual":
      return "manual";
    case "variables": {
      if (trigger.names.length === 0) return "on any variable change";
      return `on ${trigger.names.join(", ")} change`;
    }
    case "widget":
      return `on widget ${trigger.widgetId || "(unset)"} press`;
    case "pluginEvent": {
      const filter = Object.entries(trigger.filter ?? {})
        .map(([key, value]) => `${key}=${value}`)
        .join(", ");
      return `on ${trigger.plugin || "(plugin)"}:${trigger.event || "(event)"}${filter ? ` where ${filter}` : ""}`;
    }
    case "schedule":
      if (trigger.everySeconds !== undefined) return `every ${describeSeconds(trigger.everySeconds)}`;
      return `at ${trigger.at || "(time)"}${trigger.days && trigger.days.length > 0 ? ` on ${trigger.days.join(", ")}` : " every day"}`;
  }
}

function describeSeconds(seconds: number): string {
  if (seconds % 3600 === 0) return seconds === 3600 ? "hour" : `${seconds / 3600} hours`;
  if (seconds % 60 === 0) return seconds === 60 ? "minute" : `${seconds / 60} minutes`;
  return `${seconds} seconds`;
}

/**
 * One line of human-readable text that names what a block does.
 *
 * The block list shows this next to the Remove button, so a glance tells the
 * user "wait 1s, set foo to 42" without expanding the editor.
 */
export function describeBlock(block: Block): string {
  switch (block.block) {
    case "if":
      return `if ${describeCondition(block.condition)} → ${block.then.length} step${block.then.length === 1 ? "" : "s"}`;
    case "repeat":
      return `repeat ${describeValue(block.times)}× (${block.body.length} step${block.body.length === 1 ? "" : "s"})`;
    case "wait":
      return `wait ${describeValue(block.ms)}ms`;
    case "setVariable":
      return `set ${block.name || "(unnamed)"} = ${describeValue(block.value)}`;
    case "doAction":
      return `do ${block.plugin || "(plugin)"}:${block.action || "(action)"}${describeActionArgs(block.args)}`;
  }
}

function describeCondition(condition: Condition): string {
  switch (condition.kind) {
    case "compare":
      return `${describeValue(condition.left)} ${opName(condition.op)} ${describeValue(condition.right)}`;
    case "all":
      return condition.conditions.length === 0
        ? "all (empty)"
        : `all(${condition.conditions.map(describeCondition).join(", ")})`;
    case "any":
      return condition.conditions.length === 0
        ? "any (empty)"
        : `any(${condition.conditions.map(describeCondition).join(", ")})`;
    case "not":
      return `not(${describeCondition(condition.condition)})`;
  }
}

function opName(op: CompareOp): string {
  switch (op) {
    case "eq":
      return "==";
    case "notEq":
      return "!=";
    case "greater":
      return ">";
    case "greaterOrEqual":
      return ">=";
    case "less":
      return "<";
    case "lessOrEqual":
      return "<=";
    case "contains":
      return "contains";
  }
}

function describeValue(value: ValueRef): string {
  if ("variable" in value) return `$${value.variable}`;
  if ("bool" in value) return String(value.bool);
  if ("number" in value) return String(value.number);
  if ("input" in value) return `the run's ${value.input}`;
  return JSON.stringify(value.text);
}

function describeActionArgs(args: Record<string, ValueRef>): string {
  const keys = Object.keys(args);
  if (keys.length === 0) return "";
  const parts = keys.map((k) => `${k}=${describeValue(args[k])}`);
  return ` (${parts.join(", ")})`;
}