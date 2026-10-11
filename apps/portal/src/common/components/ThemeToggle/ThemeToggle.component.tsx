import type { ThemePreference } from "@flexdeck/tokens/theme.ts";
import { Segmented } from "@flexdeck/ui";

import { useTheme } from "../../../hooks/useTheme/useTheme.hook.ts";

const CHOICES: readonly { value: ThemePreference; label: string }[] = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "system", label: "Auto" },
];

/** Light / Dark / follow the system. The choice is carried by `aria-pressed`, not the fill alone. */
export function ThemeToggle({ className }: { className?: string }) {
  const { preference, setPreference } = useTheme();
  return <Segmented label="Appearance" size="sm" value={preference} onChange={setPreference} options={CHOICES} className={className} />;
}
