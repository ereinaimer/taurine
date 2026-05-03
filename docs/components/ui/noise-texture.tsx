'use client';

import { useId, type ComponentProps } from 'react';
import { cn } from '@/lib/cn';

interface NoiseTextureProps extends ComponentProps<'svg'> {
  frequency?: number;
  noiseOpacity?: number;
  octaves?: number;
  slope?: number;
}

export function NoiseTexture({
  className,
  frequency = 0.45,
  noiseOpacity = 0.22,
  octaves = 4,
  slope = 0.18,
  ...props
}: NoiseTextureProps) {
  const filterId = useId();

  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      className={cn('pointer-events-none absolute inset-0 size-full select-none opacity-40', className)}
      {...props}
    >
      <filter id={filterId}>
        <feTurbulence
          type="fractalNoise"
          baseFrequency={frequency}
          numOctaves={octaves}
          stitchTiles="stitch"
        />
        <feColorMatrix type="saturate" values="0" />
        <feComponentTransfer>
          <feFuncR type="linear" slope={slope} />
          <feFuncG type="linear" slope={slope} />
          <feFuncB type="linear" slope={slope} />
        </feComponentTransfer>
      </filter>
      <rect width="100%" height="100%" filter={`url(#${filterId})`} opacity={noiseOpacity} />
    </svg>
  );
}
