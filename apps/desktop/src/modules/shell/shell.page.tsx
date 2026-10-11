import { ShellController } from "./controller/shell.controller.ts";
import { ShellView } from "./view/shell.view.tsx";

export function ShellPage() {
  return (
    <ShellController>
      <ShellView />
    </ShellController>
  );
}
