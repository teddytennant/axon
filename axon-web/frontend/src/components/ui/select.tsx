import { type SelectHTMLAttributes, forwardRef } from "react";
import { clsx } from "clsx";

interface SelectProps extends SelectHTMLAttributes<HTMLSelectElement> {}

export const Select = forwardRef<HTMLSelectElement, SelectProps>(
  ({ className, children, ...props }, ref) => (
    <select
      ref={ref}
      className={clsx(
        "h-9 w-full appearance-none rounded-lg border border-[#222222] bg-[#111111] px-3 pr-8 text-sm text-[#f5f5f5]",
        "transition-colors cursor-pointer",
        "hover:border-[#333333]",
        "focus:border-[#00c8c8]/50 focus:outline-none focus:ring-1 focus:ring-[#00c8c8]/20",
        "disabled:opacity-40 disabled:pointer-events-none",
        // Custom arrow via background
        "bg-[url('data:image/svg+xml;charset=utf-8,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%2212%22%20height%3D%2212%22%20viewBox%3D%220%200%2024%2024%22%20fill%3D%22none%22%20stroke%3D%22%23888%22%20stroke-width%3D%222%22%20stroke-linecap%3D%22round%22%20stroke-linejoin%3D%22round%22%3E%3Cpath%20d%3D%22m6%209%206%206%206-6%22%2F%3E%3C%2Fsvg%3E')]",
        "bg-[position:right_8px_center] bg-no-repeat",
        className,
      )}
      {...props}
    >
      {children}
    </select>
  ),
);

Select.displayName = "Select";
