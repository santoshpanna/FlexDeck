import { cva, type VariantProps } from "class-variance-authority";

/**
 * A key: the one way to press anything. Raised face, a base it travels into,
 * and an inset shadow while held — the press is in the shadow and in motion's
 * `whileTap`, so it reads even before the action answers.
 */
export const keyVariants = cva(
  [
    "relative inline-flex select-none items-center justify-center gap-2 whitespace-nowrap",
    "rounded-key border font-sans text-sm font-medium",
    "transition-[background,box-shadow,border-color,color] duration-150",
    "shadow-key active:shadow-key-pressed",
    "disabled:pointer-events-none disabled:opacity-45",
    "cursor-pointer",
  ],
  {
    variants: {
      tone: {
        default:
          "border-[var(--fd-key-line)] bg-linear-to-b from-[var(--fd-key-top)] to-[var(--fd-key-bottom)] text-ink hover:brightness-110",
        signal:
          "border-signal-hi bg-linear-to-b from-signal-hi to-signal text-signal-ink font-semibold hover:brightness-105",
        ghost: "border-transparent bg-transparent text-muted shadow-none hover:text-ink active:shadow-none",
        danger:
          "border-[var(--fd-key-line)] bg-linear-to-b from-[var(--fd-key-top)] to-[var(--fd-key-bottom)] text-bad hover:brightness-110",
      },
      size: {
        sm: "h-8 px-3 text-xs",
        md: "h-10 px-4",
        lg: "h-12 px-5 text-[15px]",
        icon: "size-9 p-0",
      },
      lit: {
        true: "border-signal-line",
        false: "",
      },
    },
    defaultVariants: { tone: "default", size: "md", lit: false },
  },
);

export type KeyVariants = VariantProps<typeof keyVariants>;
