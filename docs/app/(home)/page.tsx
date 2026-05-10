import Link from 'next/link';
import { Download, BookOpen } from 'lucide-react';
import { BlurFade } from '@/components/ui/blur-fade';
import { NoiseTexture } from '@/components/ui/noise-texture';
import { cn } from '@/lib/cn';
import { appDescription, docsRoute, gitConfig } from '@/lib/shared';

const githubUrl = `https://github.com/${gitConfig.user}/${gitConfig.repo}`;

export default function HomePage() {
  return (
    <section className="relative isolate flex h-screen h-[100svh] w-full flex-col overflow-hidden bg-fd-background">
      {/* ── Background texture ── */}
      <div aria-hidden="true" className="pointer-events-none absolute inset-0">
        <div className="absolute inset-0 bg-[radial-gradient(ellipse_60%_44%_at_50%_28%,rgba(163,177,199,0.06),transparent_70%)]" />
        <NoiseTexture
          noiseOpacity={0.14}
          frequency={0.5}
          className="opacity-30 [mask-image:radial-gradient(48rem_circle_at_center,white,transparent)]"
        />
      </div>

      {/* ── Main content ── */}
      <main className="relative z-10 flex flex-1 flex-col items-center justify-center px-5 sm:px-8">
        {/* Hero */}
        <div className="flex w-full max-w-4xl flex-col items-center text-center">

          {/* Headline */}
          <BlurFade delay={0.12}>
            <h1
              className={cn(
                'text-balance font-bold text-fd-foreground',
                'text-[2.75rem] leading-[1.1] tracking-[-0.04em]',
                'sm:text-6xl',
                'md:text-7xl',
                'lg:text-[4.5rem]',
              )}
            >
              Type less. Do more.
            </h1>
          </BlurFade>

          {/* Description */}
          <BlurFade delay={0.18}>
            <p className="mx-auto mt-6 max-w-2xl text-pretty text-[15px] leading-relaxed text-fd-muted-foreground sm:text-lg sm:leading-8">
              {appDescription}
            </p>
          </BlurFade>

          {/* CTA buttons */}
          <BlurFade delay={0.24}>
            <div className="mt-8 flex flex-col items-center justify-center gap-3 sm:flex-row sm:gap-4">
              <Link
                href={`${githubUrl}/releases`}
                target="_blank"
                rel="noreferrer"
                className={cn(
                  'group inline-flex h-12 items-center justify-center gap-2.5 rounded-xl px-8',
                  'border border-[#A3B1C7] bg-[#A3B1C7]/10',
                  'font-mono text-[12px] font-medium uppercase tracking-[0.08em] text-[#A3B1C7]',
                  'transition-all duration-150 hover:bg-[#A3B1C7] hover:text-[#141313]',
                  'active:scale-[0.97]',
                )}
              >
                <Download className="size-4" aria-hidden />
                Download Now
              </Link>
              <Link
                href={docsRoute}
                className={cn(
                  'group inline-flex h-12 items-center justify-center gap-2.5 rounded-xl px-8',
                  'border border-[#A3B1C7] bg-[#A3B1C7]/10',
                  'font-mono text-[12px] font-medium uppercase tracking-[0.08em] text-[#A3B1C7]',
                  'transition-all duration-150 hover:bg-[#A3B1C7] hover:text-[#141313]',
                  'active:scale-[0.97]',
                )}
              >
                <BookOpen className="size-4" aria-hidden />
                Read Docs
              </Link>
            </div>
          </BlurFade>
        </div>

      </main>
    </section>
  );
}
