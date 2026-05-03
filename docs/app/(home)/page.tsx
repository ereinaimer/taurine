import Link from 'next/link';
import { ArrowRight, ExternalLink } from 'lucide-react';
import { buttonVariants } from 'fumadocs-ui/components/ui/button';
import { AnimatedShinyText } from '@/components/ui/animated-shiny-text';
import { BlurFade } from '@/components/ui/blur-fade';
import { GridPattern } from '@/components/ui/grid-pattern';
import { NoiseTexture } from '@/components/ui/noise-texture';
import { cn } from '@/lib/cn';
import { appDescription, appName, docsRoute, gitConfig } from '@/lib/shared';

const githubUrl = `https://github.com/${gitConfig.user}/${gitConfig.repo}`;

export default function HomePage() {
  return (
    <section className="relative isolate flex min-h-screen min-h-[100svh] w-full items-center justify-center overflow-hidden bg-fd-background">
      <div aria-hidden="true" className="absolute inset-0 overflow-hidden">
        <div className="absolute inset-0 bg-[radial-gradient(circle_at_top,rgba(120,120,160,0.16),transparent_42%),radial-gradient(circle_at_80%_55%,rgba(255,255,255,0.07),transparent_32%)] dark:bg-[radial-gradient(circle_at_top,rgba(120,120,160,0.18),transparent_42%),radial-gradient(circle_at_80%_55%,rgba(255,255,255,0.05),transparent_30%)]" />
        <GridPattern
          width={44}
          height={44}
          squares={[
            [5, 2],
            [7, 5],
            [11, 4],
            [13, 8],
            [4, 9],
          ]}
          strokeDasharray="4 4"
          className={cn(
            'text-fd-border/60 opacity-60',
            '[mask-image:radial-gradient(56rem_circle_at_center,white,transparent)]',
            'inset-x-[-20%] inset-y-[-32%] h-[165%] w-[140%] skew-y-12',
          )}
        />
        <NoiseTexture className="[mask-image:radial-gradient(48rem_circle_at_center,white,transparent)]" />
        <div className="absolute inset-x-0 bottom-0 h-40 bg-linear-to-t from-fd-background via-fd-background/60 to-transparent" />
      </div>

      <div className="relative z-10 mx-auto flex w-full max-w-6xl flex-1 items-center justify-center px-6 py-16 sm:px-10 lg:px-16">
        <div className="flex w-full max-w-4xl flex-col items-center text-center">
          <BlurFade delay={0.04}>
            <div className="inline-flex items-center rounded-full border border-fd-border/60 bg-fd-card/45 px-4 py-1.5 backdrop-blur-md">
              <AnimatedShinyText className="text-xs font-semibold uppercase tracking-[0.34em] text-fd-muted-foreground">
                {appName}
              </AnimatedShinyText>
            </div>
          </BlurFade>

          <BlurFade delay={0.12}>
            <h1 className="mt-8 max-w-5xl text-5xl font-semibold tracking-[-0.075em] text-balance text-fd-foreground sm:text-6xl md:text-7xl lg:text-[5.6rem] lg:leading-[0.95]">
              Type less. Do more.
            </h1>
          </BlurFade>

          <BlurFade delay={0.2}>
            <p className="mx-auto mt-6 max-w-3xl text-pretty text-base leading-7 text-fd-muted-foreground sm:text-lg sm:leading-8">
              {appDescription}
            </p>
          </BlurFade>

          <BlurFade delay={0.28}>
            <div className="mt-10 flex flex-col items-center justify-center gap-3 sm:flex-row">
              <Link
                href={docsRoute}
                className={cn(
                  buttonVariants({ color: 'primary' }),
                  'h-12 rounded-full px-6 text-sm shadow-[0_10px_40px_rgba(140,170,255,0.22)]',
                )}
              >
                Get Started
                <ArrowRight className="ml-1 h-4 w-4" aria-hidden />
              </Link>
              <Link
                href={githubUrl}
                target="_blank"
                rel="noreferrer"
                className={cn(
                  buttonVariants({ color: 'secondary' }),
                  'h-12 rounded-full border-fd-border/70 bg-fd-card/50 px-6 text-sm backdrop-blur-md',
                )}
              >
                View on GitHub
                <ExternalLink className="ml-1 h-4 w-4" aria-hidden />
              </Link>
            </div>
          </BlurFade>
        </div>
      </div>
    </section>
  );
}
