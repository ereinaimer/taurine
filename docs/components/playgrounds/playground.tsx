'use client';

import React, { useState, useEffect } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { resolveTemplate } from './resolver';

interface PlaygroundProps {
  initialTemplate: string;
  initialInput?: string;
}

export function Playground({
  initialTemplate,
  initialInput
}: PlaygroundProps) {
  const [template, setTemplate] = useState(initialTemplate);
  const [input, setInput] = useState(initialInput || '');
  const [output, setOutput] = useState(initialTemplate);

  useEffect(() => {
    // When input changes, resolve the template
    const firstSpaceOrColon = input.match(/[ :]/);
    const prefix = firstSpaceOrColon ? input.slice(0, firstSpaceOrColon.index) : input;

    if (input.endsWith(' ')) {
      setOutput(resolveTemplate(template, input, prefix));
    } else {
      setOutput('Waiting for space...');
    }
  }, [template, input]);

  return (
    <div className="flex flex-col gap-4 p-6">
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
  );
}
