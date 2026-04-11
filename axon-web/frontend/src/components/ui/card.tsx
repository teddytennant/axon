import type { HTMLAttributes } from "react";
import { clsx } from "clsx";

interface CardProps extends HTMLAttributes<HTMLDivElement> {
  padding?: "none" | "sm" | "md" | "lg";
}

const paddingStyles = {
  none: "",
  sm: "p-3",
  md: "p-5",
  lg: "p-6",
};

export function Card({
  padding = "md",
  className,
  ...props
}: CardProps) {
  return (
    <div
      className={clsx(
        "rounded border border-[#1c1c1c] bg-[#0c0c0c]",
        paddingStyles[padding],
        className,
      )}
      {...props}
    />
  );
}

export function CardHeader({
  className,
  ...props
}: HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={clsx(
        "flex items-center justify-between pb-4",
        className,
      )}
      {...props}
    />
  );
}

export function CardTitle({
  className,
  ...props
}: HTMLAttributes<HTMLHeadingElement>) {
  return (
    <h3
      className={clsx("text-sm font-medium text-white", className)}
      {...props}
    />
  );
}
