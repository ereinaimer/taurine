import React from 'react';
import { ArrowRight, Terminal, Sparkles } from 'lucide-react';

interface TransformationCase {
  input: string;
  result: string;
}

interface TransformationProps {
  /** The snippet template, e.g. "Hello, [name]!" */
  template: string;
  /** Single input trigger, used when there is only one example */
  input?: string;
  /** Single result string, used when there is only one example */
  result?: string;
  /** Multiple input/result pairs for showing several cases at once */
  cases?: TransformationCase[];
}

function Badge({
  label,
  variant,
}: {
  label: string;
  variant: 'template' | 'input' | 'result';
}) {
  const styles: Record<string, string> = {
    template:
      'bg-fd-primary/10 text-fd-primary border border-fd-primary/20',
    input:
      'bg-amber-500/10 text-amber-600 border border-amber-500/20 dark:text-amber-400',
    result:
      'bg-emerald-500/10 text-emerald-600 border border-emerald-500/20 dark:text-emerald-400',
  };

  return (
    <span
      className={`inline-flex items-center rounded-md px-2 py-0.5 text-[10px] font-semibold uppercase tracking-widest ${styles[variant]}`}
    >
      {label}
    </span>
  );
}

function MonoLine({ children }: { children: React.ReactNode }) {
  return (
    <span className="font-mono text-sm text-fd-foreground break-all">
      {children}
    </span>
  );
}

function CaseRow({ input, result }: TransformationCase) {
  return (
    <div className="grid grid-cols-1 gap-2 sm:grid-cols-[1fr_auto_1fr] sm:items-center">
      {/* Input */}
      <div className="flex items-start gap-2.5 rounded-lg border border-fd-border bg-fd-muted/40 px-3.5 py-2.5">
        <Terminal
          className="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-500"
          aria-hidden
        />
        <MonoLine>{input}</MonoLine>
      </div>

      {/* Arrow */}
      <div className="hidden sm:flex items-center justify-center">
        <ArrowRight className="h-4 w-4 text-fd-muted-foreground" aria-hidden />
      </div>
      <div className="flex sm:hidden items-center justify-center my-0.5">
        <div className="h-px w-8 bg-fd-border" />
      </div>

      {/* Result */}
      <div className="flex items-start gap-2.5 rounded-lg border border-fd-border bg-fd-muted/40 px-3.5 py-2.5">
        <Sparkles
          className="mt-0.5 h-3.5 w-3.5 shrink-0 text-emerald-500"
          aria-hidden
        />
        <MonoLine>{result}</MonoLine>
      </div>
    </div>
  );
}

export function Transformation({
  template,
  input,
  result,
  cases,
}: TransformationProps) {
  const resolvedCases: TransformationCase[] =
    cases ??
    (input !== undefined && result !== undefined ? [{ input, result }] : []);

  return (
    <div className="not-prose my-5 overflow-hidden rounded-xl border border-fd-border bg-fd-card shadow-sm">
      {/* Template row */}
      <div className="flex flex-col gap-1.5 border-b border-fd-border bg-fd-muted/30 px-4 py-3">
        <Badge label="Template" variant="template" />
        <MonoLine>{template}</MonoLine>
      </div>

      {/* Input / Result pairs */}
      {resolvedCases.length > 0 && (
        <div className="flex flex-col divide-y divide-fd-border/60">
          {resolvedCases.map((c, i) => (
            <div key={i} className="px-4 py-3.5">
              {/* Column headers — show only once and only for multi-case */}
              {i === 0 && resolvedCases.length > 1 && (
                <div className="mb-2.5 grid grid-cols-1 gap-2 sm:grid-cols-[1fr_auto_1fr]">
                  <Badge label="Input" variant="input" />
                  <span className="hidden sm:block w-4" />
                  <Badge label="Result" variant="result" />
                </div>
              )}
              {/* Column headers — single case */}
              {resolvedCases.length === 1 && (
                <div className="mb-2.5 grid grid-cols-1 gap-2 sm:grid-cols-[1fr_auto_1fr]">
                  <Badge label="Input" variant="input" />
                  <span className="hidden sm:block w-4" />
                  <Badge label="Result" variant="result" />
                </div>
              )}
              <CaseRow {...c} />
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
