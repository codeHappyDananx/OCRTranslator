import * as React from "react";
import { cn } from "../../lib/utils";

export const Card = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => <div ref={ref} className={cn("card", className)} {...props} />,
);
Card.displayName = "Card";

export function CardContent({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("card-content", className)} {...props} />;
}
