import { type ComponentPropsWithoutRef, type CSSProperties } from 'react';
import { cn } from '@/lib/cn';

interface AnimatedShinyTextProps extends ComponentPropsWithoutRef<'span'> {
  shimmerWidth?: number;
}

export function AnimatedShinyText({
  children,
  className,
  shimmerWidth = 140,
  style,
  ...props
}: AnimatedShinyTextProps) {
  return (
    <span
      style={
        {
          '--shiny-width': `${shimmerWidth}px`,
          ...style,
        } as CSSProperties
      }
      className={cn(
        'animate-shiny-text bg-[length:var(--shiny-width)_100%] bg-clip-text bg-no-repeat',
        'bg-linear-to-r from-transparent via-fd-foreground/90 via-50% to-transparent',
        className,
      )}
      {...props}
    >
      {children}
    </span>
  );
}
