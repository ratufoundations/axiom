import * as React from "react";
import { cn } from "@/lib/utils";

export interface BadgeProps extends React.HTMLAttributes<HTMLDivElement> {
  variant?: "default" | "secondary" | "destructive" | "outline";
}

export function Badge({
  className,
  variant = "default",
  ...props
}: BadgeProps) {
  const variantStyles = {
    default: "bg-[#238636]/20 text-[#3fb950] border border-[#238636]/40",
    secondary: "bg-[#21262d] text-[#8b949e] border border-[#30363d]",
    destructive: "bg-[#da3633]/20 text-[#f85149] border border-[#da3633]/40",
    outline: "text-[#c9d1d9] border border-[#30363d]",
  }[variant];

  return (
    <div
      className={cn(
        "inline-flex items-center rounded px-1.5 py-0.5 text-[10px] font-mono font-semibold transition-colors",
        variantStyles,
        className
      )}
      {...props}
    />
  );
}
