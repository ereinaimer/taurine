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

function Label({ children }: { children: React.ReactNode }) {
  return (
    <span className="text-[10px] font-semibold uppercase tracking-widest text-fd-muted-foreground">
      {children}
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
    <>
      {/* ── Mobile layout: label immediately above its own box ── */}
      <div className="flex flex-col gap-3 sm:hidden">
        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-1.5">
            <Terminal className="h-3.5 w-3.5 shrink-0 text-fd-muted-foreground" aria-hidden />
            <Label>Input</Label>
          </div>
          <div className="rounded-lg border border-fd-border px-4 py-3">
            <MonoLine>{input}</MonoLine>
          </div>
        </div>

        <div className="flex justify-center">
          <div className="h-px w-8 bg-fd-border" />
        </div>

        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-1.5">
            <Sparkles className="h-3.5 w-3.5 shrink-0 text-fd-muted-foreground" aria-hidden />
            <Label>Result</Label>
          </div>
          <div className="rounded-lg border border-fd-border px-4 py-3">
            <MonoLine>{result}</MonoLine>
          </div>
        </div>
      </div>

      {/* ── Desktop layout: labels row + boxes+arrow row, columns aligned ── */}
      <div className="hidden sm:flex sm:flex-col sm:gap-2">
        {/* Labels — same 3-col template as the boxes row below */}
        <div className="grid grid-cols-[1fr_auto_1fr] gap-3">
          <div className="flex items-center gap-1.5">
            <Terminal className="h-3.5 w-3.5 shrink-0 text-fd-muted-foreground" aria-hidden />
            <Label>Input</Label>
          </div>
          <div className="w-4" /> {/* spacer matching arrow width */}
          <div className="flex items-center gap-1.5">
            <Sparkles className="h-3.5 w-3.5 shrink-0 text-fd-muted-foreground" aria-hidden />
            <Label>Result</Label>
          </div>
        </div>

        {/* Boxes — arrow sits between the two text boxes */}
        <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-3">
          <div className="rounded-lg border border-fd-border px-4 py-3">
            <MonoLine>{input}</MonoLine>
          </div>
          <ArrowRight className="h-4 w-4 text-fd-muted-foreground" aria-hidden />
          <div className="rounded-lg border border-fd-border px-4 py-3">
            <MonoLine>{result}</MonoLine>
          </div>
        </div>
      </div>
    </>
  );
}

export function Transformation({ template, input, result, cases }: TransformationProps) {
  const resolvedCases: TransformationCase[] =
    cases ?? (input !== undefined && result !== undefined ? [{ input, result }] : []);

  return (
    <div className="not-prose my-5 overflow-hidden rounded-xl border border-fd-border bg-fd-card shadow-sm">
      {/* Template */}
      <div className="flex flex-col gap-2 border-b border-fd-border px-4 py-4">
        <Label>Template</Label>
        <MonoLine>{template}</MonoLine>
      </div>

      {/* Input / Result pairs */}
      {resolvedCases.length > 0 && (
        <div className="flex flex-col divide-y divide-fd-border/60">
          {resolvedCases.map((c, i) => (
            <div key={i} className="px-4 py-4">
              <CaseRow {...c} />
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
