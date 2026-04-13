import Link from "next/link";
import { compileMDX } from "next-mdx-remote/rsc";
import remarkGfm from "remark-gfm";
import { OnThisPage } from "@/components/OnThisPage";
import { Sidebar } from "@/components/Sidebar";
import { TopNavbar } from "@/components/TopNavbar";
import { getAllSlugs, getDoc, getPrevNext, getSearchIndex, resolveSlug } from "@/lib/docs";
import { useMDXComponents } from "@/mdx-components";
import { SidebarProvider } from "@/components/ui/sidebar";

type PageProps = {
  params: Promise<{ slug?: string[] }>;
};

export async function generateStaticParams() {
  return getAllSlugs().map((slug) => ({ slug }));
}

export default async function DocsPage({ params }: PageProps) {
  const { slug } = await params;
  const currentSlug = resolveSlug(slug);
  const doc = getDoc(slug);
  const searchIndex = getSearchIndex();
  const { prev, next } = getPrevNext(currentSlug);

  const compiled = await compileMDX({
    source: doc.content,
    options: {
      mdxOptions: {
        remarkPlugins: [remarkGfm]
      },
      parseFrontmatter: false
    },
    components: useMDXComponents({})
  });

  return (
    <div className="min-h-screen bg-white text-[15px] text-ink dark:bg-ink dark:text-cloud">
      <SidebarProvider className="block w-full">
        <TopNavbar searchIndex={searchIndex} />
        <Sidebar currentSlug={currentSlug} />
        <div className="flex pl-0 md:pl-[260px]">
          <main className="w-full px-8 py-10 lg:px-10">
            <article className="mx-auto max-w-prose">
              <h1 className="text-[28px] font-semibold">{doc.title}</h1>
              <p className="mt-2 text-sm italic text-slate dark:text-slate-300">{doc.description}</p>
              <div className="mt-8">{compiled.content}</div>

              <nav className="mt-14 grid gap-3 border-t border-slate-200 pt-6 dark:border-slate-800/30 sm:grid-cols-2">
                {prev ? (
                  <Link
                    href={`/docs/${prev.slug}`}
                    className="rounded border border-slate-300 p-3 text-sm hover:border-ember dark:border-slate-800/40"
                  >
                    <span className="block text-xs text-slate">Previous</span>
                    {prev.title}
                  </Link>
                ) : (
                  <span />
                )}
                {next ? (
                  <Link
                    href={`/docs/${next.slug}`}
                    className="rounded border border-slate-300 p-3 text-right text-sm hover:border-ember dark:border-slate-800/40"
                  >
                    <span className="block text-xs text-slate">Next</span>
                    {next.title}
                  </Link>
                ) : (
                  <span />
                )}
              </nav>
            </article>
          </main>

          <OnThisPage headings={doc.headings} />
        </div>
      </SidebarProvider>
    </div>
  );
}
