'use client';

import React, { useState, type ReactNode } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { Play, Square } from 'lucide-react';
import { CodeBlock, Pre } from 'fumadocs-ui/components/codeblock';
import { Playground } from './playground';

// Simple cn utility since we can't easily import it from fumadocs-ui/dist
function cn(...classes: (string | boolean | undefined)[]) {
  return classes.filter(Boolean).join(' ');
}

interface PlayableCodeBlockProps {
  children?: ReactNode;
  trigger?: string;
  'data-trigger'?: string;
  title?: string;
  className?: string;
  [key: string]: any;
}

/**
 * Extracts raw text from React children (usually a <code> tag inside <pre>)
 */
function extractText(node: ReactNode): string {
  if (typeof node === 'string') return node;
  if (typeof node === 'number') return String(node);
  if (Array.isArray(node)) return node.map(extractText).join('');
  if (React.isValidElement(node) && node.props && typeof node.props === 'object' && 'children' in node.props) {
    return extractText((node.props as any).children as ReactNode);
  }
  return '';
}

export function PlayableCodeBlock({ 
  children, 
  trigger, 
  'data-trigger': dataTrigger,
  title, 
  className, 
  ...props 
}: PlayableCodeBlockProps) {
  const [isPlaying, setIsPlaying] = useState(false);
  const resolvedTrigger = trigger ?? dataTrigger;
  const rawContent = extractText(children).trim();

  // We always wrap in CodeBlock to ensure titles and Copy buttons appear
  // This matches Fumadocs' default MDX behavior for the 'pre' tag
  return (
    <div className={cn("relative group/playable my-6", isPlaying && "is-playing")}>
      <CodeBlock
        title={title}
        {...props}
        className={cn(className, isPlaying && "mb-0 rounded-b-none")}
        Actions={resolvedTrigger ? ({ className: actionsClass, children: actionsChildren }) => (
          <div className={cn("flex items-center gap-1.5", actionsClass)}>
            <button
              onClick={() => setIsPlaying(!isPlaying)}
              className="flex h-7 w-7 items-center justify-center rounded-md border border-fd-border bg-fd-secondary/50 text-fd-muted-foreground hover:bg-fd-secondary hover:text-fd-foreground transition-all"
              title={isPlaying ? "Stop Simulation" : "Play Simulation"}
            >
              {isPlaying ? (
                <Square className="h-3 w-3 fill-current" />
              ) : (
                <Play className="h-3 w-3 fill-current" />
              )}
            </button>
            {actionsChildren}
          </div>
        ) : undefined}
      >
        <Pre>{children}</Pre>
      </CodeBlock>

      <AnimatePresence>
        {isPlaying && (
          <motion.div
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: 'auto' }}
            exit={{ opacity: 0, height: 0 }}
            className="overflow-hidden border-x border-b border-fd-border rounded-b-xl bg-fd-card/30 backdrop-blur-md"
          >
            <div className="p-1 pt-0">
              <Playground 
                initialTemplate={rawContent} 
                initialInput={resolvedTrigger} 
                showToggle={false} 
              />
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
