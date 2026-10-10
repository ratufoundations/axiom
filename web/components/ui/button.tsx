import * as React from "react";
import { cn } from "@/lib/utils";

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "default" | "destructive" | "outline" | "secondary" | "ghost" | "link";
  size?: "default" | "sm" | "lg" | "icon";
}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = "default", size = "default", ...props }, ref) => {
    const variantStyles = {
      default: "bg-[#238636] text-white hover:bg-[#2ea043] shadow-xs",
      destructive: "bg-[#da3633] text-white hover:bg-[#b62324]",
      outline: "border border-[#30363d] bg-transparent hover:bg-[#21262d] text-[#c9d1d9]",
      secondary: "bg-[#21262d] text-[#c9d1d9] hover:bg-[#30363d]",
      ghost: "hover:bg-[#21262d] text-[#c9d1d9] hover:text-[#f0f6fc]",
      link: "text-[#58a6ff] underline-offset-4 hover:underline",
    }[variant];

    const sizeStyles = {
      default: "h-9 px-4 py-2 text-sm",
      sm: "h-7 rounded px-2.5 text-xs",
      lg: "h-10 rounded px-6 text-base",
      icon: "h-7 w-7 p-0 flex items-center justify-center",
    }[size];

    return (
      <button
        ref={ref}
        className={cn(
          "inline-flex items-center justify-center gap-1.5 rounded font-mono font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[#58a6ff] disabled:pointer-events-none disabled:opacity-50 cursor-pointer",
          variantStyles,
          sizeStyles,
          className
        )}
        {...props}
      />
    );
  }
);
Button.displayName = "Button";
