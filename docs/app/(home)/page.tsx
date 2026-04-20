import Link from 'next/link';
import { appDescription, appName } from '@/lib/shared';

export default function HomePage() {
  return (
    <div className="flex flex-1 flex-col items-center justify-center px-6 text-center">
      <h1 className="mb-4 text-4xl font-bold">{appName}</h1>
      <p className="mb-8 max-w-2xl text-fd-muted-foreground">{appDescription}</p>
      <p>
        Open{' '}
        <Link href="/docs" className="font-medium underline">
          /docs
        </Link>{' '}
        to browse the documentation.
      </p>
    </div>
  );
}
