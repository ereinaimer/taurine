'use client';

import React, { useState, type ChangeEvent } from 'react';

export function InteractiveExpander() {
  const triggerChar = '>';
  const [triggerWord, setTriggerWord] = useState('hw');
  const [expansion, setExpansion] = useState('Hello World!');
  const [text, setText] = useState('');

  const handleTextChange = (e: ChangeEvent<HTMLTextAreaElement>) => {
    const newValue = e.target.value;
    const trigger = triggerChar + triggerWord + ' ';

    if (newValue.endsWith(trigger)) {
      const expandedValue = newValue.slice(0, -trigger.length) + expansion;
      setText(expandedValue);
    } else {
      setText(newValue);
    }
  };

  return (
    <div className="my-6 flex flex-col gap-4 rounded-xl border border-fd-border bg-fd-card p-6 shadow-sm">
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
        <div className="flex flex-col gap-1.5 sm:col-span-1">
          <label className="text-xs font-medium text-fd-muted-foreground uppercase tracking-wider">
            Trigger Word
          </label>
          <div className="flex gap-2">
            <span className="flex items-center justify-center rounded-md border border-fd-border bg-fd-muted px-3 text-sm font-mono text-fd-muted-foreground">
              {triggerChar}
            </span>
            <input
              type="text"
              value={triggerWord}
              onChange={(e) => setTriggerWord(e.target.value)}
              className="w-full rounded-md border border-fd-border bg-fd-background px-3 py-2 text-sm text-fd-foreground focus:outline-none focus:ring-1 focus:ring-fd-primary/30 transition-all"
              placeholder="e.g. gn"
            />
          </div>
        </div>
        <div className="flex flex-col gap-1.5 sm:col-span-2">
          <label className="text-xs font-medium text-fd-muted-foreground uppercase tracking-wider">
            Expansion
          </label>
          <input
            type="text"
            value={expansion}
            onChange={(e) => setExpansion(e.target.value)}
            className="w-full rounded-md border border-fd-border bg-fd-background px-3 py-2 text-sm text-fd-foreground focus:outline-none focus:ring-1 focus:ring-fd-primary/30 transition-all"
            placeholder="e.g. Good night!"
          />
        </div>
      </div>

      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-fd-muted-foreground uppercase tracking-wider">
          Testing Area
        </label>
        <textarea
          value={text}
          onChange={handleTextChange}
          placeholder={`Type "${triggerChar}${triggerWord}" followed by a space to see it expand...`}
          className="min-h-[120px] w-full resize-none rounded-md border border-fd-border bg-fd-background p-3 text-sm text-fd-foreground focus:outline-none focus:ring-1 focus:ring-fd-primary/30 transition-all"
        />
      </div>
    </div>
  );
}
