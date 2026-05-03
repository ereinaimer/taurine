'use client';

import { motion, type HTMLMotionProps } from 'framer-motion';

type Direction = 'up' | 'down' | 'left' | 'right';

interface BlurFadeProps extends HTMLMotionProps<'div'> {
  blur?: string;
  delay?: number;
  direction?: Direction;
  duration?: number;
  offset?: number;
}

export function BlurFade({
  animate,
  blur = '10px',
  children,
  delay = 0,
  direction = 'down',
  duration = 0.55,
  initial,
  offset = 16,
  transition,
  ...props
}: BlurFadeProps) {
  const axis = direction === 'left' || direction === 'right' ? 'x' : 'y';
  const delta = direction === 'down' || direction === 'right' ? -offset : offset;
  const hidden = {
    opacity: 0,
    filter: `blur(${blur})`,
    ...(axis === 'x' ? { x: delta, y: 0 } : { x: 0, y: delta }),
  };

  return (
    <motion.div
      initial={initial ?? hidden}
      animate={animate ?? { x: 0, y: 0, opacity: 1, filter: 'blur(0px)' }}
      transition={
        transition ?? {
          delay,
          duration,
          ease: [0.16, 1, 0.3, 1],
        }
      }
      {...props}
    >
      {children}
    </motion.div>
  );
}
