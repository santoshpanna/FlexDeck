/**
 * Test a plugin without the agent.
 *
 * {@link harness} drives a plugin the way the agent does — the same methods,
 * the same answers — and keeps what the plugin sent back, so a test reads like
 * "press greet, then the count is 1 and an event fired". {@link checkDefinition}
 * applies the agent's own acceptance rules, so a plugin that would be refused
 * at install fails its own tests first.
 */

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import type { Plugin, Value } from "./index.ts";
import {
  ABI_VERSION,
  type ChoicesResponse,
  type InvokeResponse,
  isValidId,
  Method,
  Notify,
  type PackageManifest,
  SUPPORTED_ABIS,
  type ValueDto,
} from "./protocol.ts";

export interface Host {
  /**
   * Run an action; throws with the plugin's message if it fails. Resolves to
   * the look the action reported, if any.
   */
  invoke(action: string, args?: Record<string, string>): Promise<string | undefined>;
  /** The plugin's reason for refusing these arguments, or `undefined`. */
  check(action: string, args?: Record<string, string>): Promise<string | undefined>;
  /** What the editor would be offered for `action`'s arguments, given `args`. */
  choices(action: string, args?: Record<string, string>): Promise<ChoicesResponse>;
  /** Ask for values, as the agent's sampler would. */
  sample(wanted: string[]): Promise<Record<string, Value>>;
  /** Change settings, as the person saving the form would. */
  changeSettings(settings: Record<string, string>): Promise<void>;
  /** The last value the plugin published for `name`. */
  variable(name: string): Value | undefined;
  events(): { event: string; payload: Record<string, string> }[];
  notifications(): { title: string; body: string }[];
  logs(): { level: string; message: string }[];
  /** Every `settings/store` the plugin sent, merged in order. */
  stored(): Record<string, string>;
  /** The addresses the plugin asked the agent to open. */
  openedUrls(): string[];
  /** The images the plugin handed over, by sha256. */
  images(): { sha256: string; mime: string; data: string }[];
  /** A key starts using `feed` as `instance`, with `args` (ABI 4). */
  watch(instance: string, feed: string, args?: Record<string, string>): Promise<void>;
  /** That key goes away (ABI 4). */
  unwatch(instance: string): Promise<void>;
  /** What the plugin published for one key's feed: `{temp: 21}` for `<id>.@<instance>.temp`. */
  feed(instance: string): Record<string, Value>;
  /** The last whole history the plugin sent for `name` (ABI 4). */
  series(name: string): number[] | undefined;
}

export interface HarnessOptions {
  settings?: Record<string, string>;
  /** Run the plugin's `start` too. Off by default: start usually connects out. */
  start?: boolean;
}

export async function harness(plugin: Plugin, options: HarnessOptions = {}): Promise<Host> {
  const from = plugin.sent.length;
  plugin.configure(options.settings ?? {});
  // Speak the newest ABI, as a current agent would, without running `start`.
  plugin.useAbi(ABI_VERSION);
  if (options.start) await plugin.definition.start?.(plugin.context);

  const since = (method: string) =>
    plugin.sent.slice(from).filter((message) => message.method === method).map((message) => message.params);

  return {
    async invoke(action, args = {}) {
      const answer = (await plugin.handle(Method.invoke, { action, args })) as InvokeResponse;
      if (!answer.ok) throw new Error(answer.error ?? `${action} failed`);
      return answer.state;
    },
    async check(action, args = {}) {
      const answer = (await plugin.handle(Method.check, { action, args })) as { ok: boolean; error?: string };
      return answer.ok ? undefined : answer.error;
    },
    async choices(action, args = {}) {
      return (await plugin.handle(Method.choices, { action, args })) as ChoicesResponse;
    },
    async sample(wanted) {
      const answer = (await plugin.handle(Method.sample, { wanted })) as { values: Record<string, ValueDto> };
      return Object.fromEntries(Object.entries(answer.values).map(([name, dto]) => [name, dto.value]));
    },
    async changeSettings(settings) {
      await plugin.handle(Method.settingsChanged, { settings });
    },
    variable(name) {
      return plugin.context.vars.get(name);
    },
    events: () => since(Notify.eventEmit) as { event: string; payload: Record<string, string> }[],
    notifications: () => since(Notify.notify) as { title: string; body: string }[],
    logs: () => since(Notify.log) as { level: string; message: string }[],
    stored: () =>
      Object.assign({}, ...(since(Notify.settingsStore) as { values: Record<string, string> }[]).map((p) => p.values)),
    openedUrls: () => (since(Notify.openUrl) as { url: string }[]).map((p) => p.url),
    images: () => since(Notify.imagesPut) as { sha256: string; mime: string; data: string }[],
    async watch(instance, feed, args = {}) {
      await plugin.handle(Method.feedWatch, { instance, feed, args });
    },
    async unwatch(instance) {
      await plugin.handle(Method.feedUnwatch, { instance });
    },
    feed(instance) {
      const prefix = `${plugin.definition.id}.@${instance}.`;
      const values: Record<string, Value> = {};
      for (const set of since(Notify.varsSet) as { values: Record<string, ValueDto> }[]) {
        for (const [name, dto] of Object.entries(set.values)) {
          if (name.startsWith(prefix)) values[name.slice(prefix.length)] = dto.value;
        }
      }
      return values;
    },
    series(name) {
      const sets = (since(Notify.seriesSet) as { name: string; points: number[] }[]).filter((set) => set.name === name);
      return sets.at(-1)?.points;
    },
  };
}

export interface CheckOptions {
  /**
   * Also run every action with its own example arguments. Off for a plugin
   * whose actions need something outside the test, such as a running OBS.
   */
  run?: boolean;
}

/** Fail unless the agent would accept this plugin, and its package agrees. */
export async function checkDefinition(
  plugin: Plugin,
  packageJson?: URL | string,
  options: CheckOptions = {},
): Promise<void> {
  const declared = packageJson
    ? (JSON.parse(readFileSync(packageJson, "utf8")) as PackageManifest)
    : undefined;
  const abi = declared?.abi ?? ABI_VERSION;
  assert.ok(SUPPORTED_ABIS.includes(abi), `plugin.json names ABI ${abi}; the agent speaks ${SUPPORTED_ABIS.join(" or ")}`);
  const manifest = plugin.manifest(abi);
  assert.ok(isValidId(manifest.id), `'${manifest.id}' is not a usable id`);
  const names = new Set<string>();
  for (const action of manifest.actions) {
    assert.ok(!names.has(action.action), `action '${action.action}' is declared twice`);
    names.add(action.action);
    for (const argument of action.arguments) {
      assert.ok(argument.example !== "" || argument.optional, `${action.action}.${argument.name} needs an example`);
    }
  }
  for (const variable of plugin.variables()) {
    assert.ok(variable.name.startsWith(`${manifest.id}.`), `'${variable.name}' is outside '${manifest.id}.'`);
  }

  if (declared) {
    assert.equal(declared.id, manifest.id, "plugin.json and the code disagree about the id");
    assert.equal(declared.version, manifest.version, "plugin.json and the code disagree about the version");
    for (const permission of manifest.permissions) {
      assert.ok(
        (declared.permissions ?? []).includes(permission),
        `the code asks for '${permission}', which plugin.json does not declare`,
      );
    }
  }

  if (options.run !== false) {
    const host = await harness(plugin);
    for (const action of manifest.actions) {
      const args = Object.fromEntries(action.arguments.map((argument) => [argument.name, argument.example]));
      assert.equal(await host.check(action.action, args), undefined, `${action.action} refuses its own example`);
      await host.invoke(action.action, args);
    }
  }
}
