export default function Layout({ children }: LayoutProps<'/'>) {
  return <div className="flex min-h-screen min-h-[100svh] flex-1 overflow-hidden" suppressHydrationWarning>{children}</div>;
}
