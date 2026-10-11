import { useContext } from "react";

import {
  DesktopContext,
  type DesktopValue,
} from "../../providers/DesktopProvider/DesktopProvider.provider.tsx";

export function useDesktops(): DesktopValue {
  const value = useContext(DesktopContext);
  if (!value) {
    throw new Error("useDesktops must be used inside <DesktopProvider>");
  }
  return value;
}
