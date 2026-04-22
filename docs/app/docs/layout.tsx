import { source } from '@/lib/source';
import { DocsLayout } from 'fumadocs-ui/layouts/docs';
import { baseOptions } from '@/lib/layout.shared';
import { GithubInfo } from 'fumadocs-ui/components/github-info';
import { gitConfig } from '@/lib/shared';

export default function Layout({ children }: { children: React.ReactNode }) {
  return (
    <DocsLayout 
      tree={source.getPageTree()} 
      {...baseOptions()}
      links={[
        {
          type: 'custom',
          children: (
            <GithubInfo 
              owner={gitConfig.user} 
              repo={gitConfig.repo} 
              className="mt-auto"
            />
          ),
        },
      ]}
    >
      {children}
    </DocsLayout>
  );
}
