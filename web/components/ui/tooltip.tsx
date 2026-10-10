"use client";

import * as React from "react";
import { cn } from "@/lib/utils";

export function TooltipProvider({ children }: { children: React.ReactNode }) {
  return <>{children}</>;
}

export function Tooltip({ children }: { children: React.ReactNode }) {
  return <div className="relative group inline-block">{children}</div>;
}

export function TooltipTrigger({
  children,
}: {
  children: React.ReactNode;
  asChild?: boolean;
}) {
  return <>{children}</>;
}

export function TooltipContent({
  className,
  children,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn(
        "absolute z-50 hidden group-hover:flex bottom-full mb-1 left-1/2 -translate-x-1/2 whitespace-nowrap rounded border border-[#30363d] bg-[#161b22] px-2 py-1 text-[11px] font-mono text-[#c9d1d9] shadow-md",
        className
      )}
      {...props}
    >
      {children}
    </div>
  );
}
