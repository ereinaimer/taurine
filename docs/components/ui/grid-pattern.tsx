import { useId, type SVGProps } from 'react';
import { cn } from '@/lib/cn';

interface GridPatternProps extends SVGProps<SVGSVGElement> {
  height?: number;
  squares?: Array<[number, number]>;
  strokeDasharray?: string;
  width?: number;
  x?: number;
  y?: number;
}

export function GridPattern({
  className,
  height = 40,
  squares,
  strokeDasharray = '0',
  width = 40,
  x = -1,
  y = -1,
  ...props
}: GridPatternProps) {
  const id = useId();

  return (
    <svg
      aria-hidden="true"
      className={cn(
        'pointer-events-none absolute inset-0 h-full w-full fill-current stroke-current',
        className,
      )}
      {...props}
    >
      <defs>
        <pattern
          id={id}
          width={width}
          height={height}
          patternUnits="userSpaceOnUse"
          x={x}
          y={y}
        >
          <path d={`M.5 ${height}V.5H${width}`} fill="none" strokeDasharray={strokeDasharray} />
        </pattern>
      </defs>
      <rect width="100%" height="100%" strokeWidth={0} fill={`url(#${id})`} />
      {squares ? (
        <svg x={x} y={y} className="overflow-visible">
          {squares.map(([squareX, squareY]) => (
            <rect
              key={`${squareX}-${squareY}`}
              width={width - 1}
              height={height - 1}
              x={squareX * width + 1}
              y={squareY * height + 1}
              strokeWidth={0}
            />
          ))}
        </svg>
      ) : null}
    </svg>
  );
}
