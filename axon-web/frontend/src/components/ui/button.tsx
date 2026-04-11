import { type ButtonHTMLAttributes, forwardRef } from "react";
import { clsx } from "clsx";

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
type ButtonSize = "sm" | "md" | "lg";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
}

const variantStyles: Record<ButtonVariant, string> = {
  primary:
    "bg-white text-black hover:bg-[#e8e8e8] active:bg-[#d0d0d0]",
  secondary:
    "bg-transparent text-[#f5f5f5] border border-[#1c1c1c] hover:border-[#2a2a2a] hover:bg-[#141414]",
  ghost:
    "bg-transparent text-[#6b6b6b] hover:text-[#f5f5f5] hover:bg-[#141414]",
  danger:
    "bg-transparent text-[#ef4444] border border-[#ef4444]/20 hover:bg-[#ef4444]/8",
};

const sizeStyles: Record<ButtonSize, string> = {
  sm: "h-7 px-2.5 text-xs gap-1.5",
  md: "h-8 px-3.5 text-sm gap-2",
  lg: "h-10 px-5 text-sm gap-2",
};

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ variant = "secondary", size = "md", className, disabled, ...props }, ref) => (
    <button
      ref={ref}
      disabled={disabled}
      className={clsx(
        "inline-flex items-center justify-center rounded font-medium transition-colors cursor-pointer",
        "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-white/20",
        "disabled:opacity-30 disabled:pointer-events-none",
        variantStyles[variant],
        sizeStyles[size],
        className,
      )}
      {...props}
    />
  ),
);

Button.displayName = "Button";
