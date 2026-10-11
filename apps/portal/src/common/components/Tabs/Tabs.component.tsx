import { Segmented } from "@flexdeck/ui";

export interface TabItem<T extends string = string> {
  id: T;
  label: string;
}

export interface TabsProps<T extends string = string> {
  tabs: readonly TabItem<T>[];
  active: T;
  onSelect: (id: T) => void;
  className?: string;
}

/** Sections, as a recessed track with one pill that springs to the current one. */
export function Tabs<T extends string = string>({ tabs, active, onSelect, className }: TabsProps<T>) {
  return (
    <Segmented
      label="Sections"
      value={active}
      onChange={onSelect}
      options={tabs.map((tab) => ({ value: tab.id, label: tab.label }))}
      className={className}
    />
  );
}
