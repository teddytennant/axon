import { type InputHTMLAttributes, forwardRef } from "react";
import { clsx } from "clsx";

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {}

export const Input = forwardRef<HTMLInputElement, InputProps>(
  ({ className, ...props }, ref) => (
    <input
      ref={ref}
      className={clsx(
        "h-9 w-full rounded-lg border border-[#222222] bg-[#111111] px-3 text-sm text-[#f5f5f5]",
        "placeholder:text-[#555555]",
        "transition-colors",
        "hover:border-[#333333]",
        "focus:border-[#00c8c8]/50 focus:outline-none focus:ring-1 focus:ring-[#00c8c8]/20",
        "disabled:opacity-40 disabled:pointer-events-none",
        className,
      )}
      {...props}
    />
  ),
);

Input.displayName = "Input";
