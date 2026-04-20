'use client';

import React, { useState, useEffect } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { resolveTemplate } from './resolver';

interface PlaygroundProps {
  initialTemplate: string;
  initialInput?: string;
}

export function Playground({ initialTemplate, initialInput }: PlaygroundProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [template, setTemplate] = useState(initialTemplate);
  const [input, setInput] = useState(initialInput || '');
  const [output, setOutput] = useState(initialTemplate);

  useEffect(() => {
    // When input changes, resolve the template
    // Assuming the trigger prefix is what's before the first colon or space
    // Let's deduce prefix from initialInput if possible, otherwise just use >test
    const firstSpaceOrColon = input.match(/[ :]/);
    const prefix = firstSpaceOrColon ? input.slice(0, firstSpaceOrColon.index) : input;
    
    // We only resolve if the input ends with a space (Taurine's way)
    if (input.endsWith(' ')) {
      setOutput(resolveTemplate(template, input, prefix));
    } else {
      setOutput('Waiting for space...\\n(Type space to trigger)');
    }
  }, [template, input]);

  return (
    <div className="my-6">
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="flex items-center gap-2 rounded-lg border border-fd-border bg-fd-secondary/50 px-4 py-2 text-sm font-medium text-fd-foreground hover:bg-fd-secondary transition-colors"
      >
        <span>{isOpen ? 'Close Playground' : 'Try it!'}</span>
        <svg
          className={`h-4 w-4 transition-transform ${isOpen ? 'rotate-180' : ''}`}
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
        </svg>
      </button>

      <AnimatePresence>
        {isOpen && (
          <motion.div
            initial={{ opacity: 0, height: 0, marginTop: 0 }}
            animate={{ opacity: 1, height: 'auto', marginTop: 16 }}
            exit={{ opacity: 0, height: 0, marginTop: 0 }}
            className="overflow-hidden"
          >
            <div className="flex flex-col gap-4 rounded-xl border border-fd-border bg-fd-card p-6 shadow-sm">
              <div className="grid grid-cols-1 gap-6 sm:grid-cols-2">
                <div className="flex flex-col gap-2">
                  <label className="text-xs font-semibold uppercase tracking-wider text-fd-muted-foreground">
                    User Input
                  </label>
                  <textarea
                    value={input}
                    onChange={(e) => setInput(e.target.value)}
                    className="min-h-[100px] w-full resize-none rounded-md border border-fd-border bg-fd-background p-3 text-sm font-mono text-fd-foreground focus:outline-none focus:ring-1 focus:ring-fd-primary/30 transition-all"
                  />
                  <p className="text-xs text-fd-muted-foreground">
                    Type a space at the end to trigger expansion.
                  </p>
                </div>

                <div className="flex flex-col gap-2">
                  <label className="text-xs font-semibold uppercase tracking-wider text-fd-muted-foreground">
                    Template
                  </label>
                  <textarea
                    value={template}
                    onChange={(e) => setTemplate(e.target.value)}
                    className="min-h-[100px] w-full resize-none rounded-md border border-fd-border bg-fd-background p-3 text-sm font-mono text-fd-foreground focus:outline-none focus:ring-1 focus:ring-fd-primary/30 transition-all"
                  />
                  <p className="text-xs text-fd-muted-foreground">
                    Define variables like <code className="text-fd-primary">{'[name]'}</code> or <code className="text-fd-primary">{'[0]'}</code>.
                  </p>
                </div>
              </div>

              <div className="flex flex-col gap-2 border-t border-fd-border pt-4">
                <label className="text-xs font-semibold uppercase tracking-wider text-fd-muted-foreground">
                  Result
                </label>
                <div className="min-h-[60px] w-full whitespace-pre-wrap rounded-md bg-fd-muted p-4 text-sm font-mono text-fd-foreground border border-fd-border/50">
                  {output}
                </div>
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
