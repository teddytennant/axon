import type { HTMLAttributes } from "react";
import { clsx } from "clsx";

type BadgeVariant = "default" | "accent" | "success" | "warning" | "error";

interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  variant?: BadgeVariant;
}

const variantStyles: Record<BadgeVariant, string> = {
  default: "bg-[#181818] text-[#888888] border-[#222222]",
  accent: "bg-[#00c8c8]/10 text-[#00c8c8] border-[#00c8c8]/20",
  success: "bg-[#50dc78]/10 text-[#50dc78] border-[#50dc78]/20",
  warning: "bg-[#f0c83c]/10 text-[#f0c83c] border-[#f0c83c]/20",
  error: "bg-[#f05050]/10 text-[#f05050] border-[#f05050]/20",
};

export function Badge({
  variant = "default",
  className,
  ...props
}: BadgeProps) {
  return (
    <span
      className={clsx(
        "inline-flex items-center rounded-md border px-2 py-0.5 text-xs font-medium",
        variantStyles[variant],
        className,
      )}
      {...props}
    />
  );
}
