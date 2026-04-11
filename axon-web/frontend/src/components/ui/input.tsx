import { type InputHTMLAttributes, forwardRef } from "react";
import { clsx } from "clsx";

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {}

export const Input = forwardRef<HTMLInputElement, InputProps>(
  ({ className, ...props }, ref) => (
    <input
      ref={ref}
      className={clsx(
        "h-8 w-full rounded border border-[#1c1c1c] bg-[#0c0c0c] px-3 text-sm text-white",
        "placeholder:text-[#3a3a3a]",
        "transition-colors",
        "hover:border-[#2a2a2a]",
        "focus:border-[#3a3a3a] focus:outline-none focus:ring-1 focus:ring-white/10",
        "disabled:opacity-30 disabled:pointer-events-none",
        className,
      )}
      {...props}
    />
  ),
);

Input.displayName = "Input";
