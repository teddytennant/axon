import type { HTMLAttributes } from "react";
import { clsx } from "clsx";

type BadgeVariant = "default" | "accent" | "success" | "warning" | "error";

interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  variant?: BadgeVariant;
}

const variantStyles: Record<BadgeVariant, string> = {
  default: "bg-transparent text-[#6b6b6b] border-[#1c1c1c]",
  accent:  "bg-transparent text-[#aaaaaa] border-[#2a2a2a]",
  success: "bg-[#22c55e]/8 text-[#22c55e] border-[#22c55e]/20",
  warning: "bg-[#f59e0b]/8 text-[#f59e0b] border-[#f59e0b]/20",
  error:   "bg-[#ef4444]/8 text-[#ef4444] border-[#ef4444]/20",
};

export function Badge({
  variant = "default",
  className,
  ...props
}: BadgeProps) {
  return (
    <span
      className={clsx(
        "inline-flex items-center rounded border px-2 py-0.5 text-xs font-medium",
        variantStyles[variant],
        className,
      )}
      {...props}
    />
  );
}
