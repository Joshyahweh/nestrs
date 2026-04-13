import type { MDXComponents } from "mdx/types";
import { CodeBlock } from "@/components/mdx/CodeBlock";
import { CodeTabs } from "@/components/mdx/CodeTabs";
import { AutoCodeTabs } from "@/components/mdx/AutoCodeTabs";
import { Hint, Info, Warning } from "@/components/mdx/Callout";

type PreProps = {
  children?: {
    props?: {
      className?: string;
      children?: string;
      filename?: string;
      metastring?: string;
    };
  };
};

const headingAnchor = "group scroll-mt-28 text-ink dark:text-cloud";

export function useMDXComponents(components: MDXComponents): MDXComponents {
  return {
    ...components,
    pre: ({ children }: PreProps) => {
      const className = children?.props?.className ?? "";
      const language = className.replace("language-", "") || "text";
      const code = children?.props?.children ?? "";
      const metastring = children?.props?.metastring ?? "";
      const filename = children?.props?.filename ?? metastring.match(/filename="([^"]+)"/)?.[1];
      return <CodeBlock code={code} language={language} filename={filename} />;
    },
    code: ({ children }) => <code className="rounded bg-ember/10 px-1.5 py-0.5 font-mono text-ember">{children}</code>,
    h2: ({ children }) => {
      const text = String(children);
      const id = text
        .toLowerCase()
        .replace(/[^a-z0-9\s-]/g, "")
        .trim()
        .replace(/\s+/g, "-");
      return (
        <h2 id={id} className={`${headingAnchor} mt-10 text-[21px] font-medium`}>
          <a href={`#${id}`} className="inline-flex items-center gap-2 hover:underline">
            <span className="invisible text-ember group-hover:visible">#</span>
            {children}
          </a>
        </h2>
      );
    },
    h3: ({ children }) => {
      const text = String(children);
      const id = text
        .toLowerCase()
        .replace(/[^a-z0-9\s-]/g, "")
        .trim()
        .replace(/\s+/g, "-");
      return (
        <h3 id={id} className={`${headingAnchor} mt-8 text-[17px] font-medium`}>
          <a href={`#${id}`} className="inline-flex items-center gap-2 hover:underline">
            <span className="invisible text-ember group-hover:visible">#</span>
            {children}
          </a>
        </h3>
      );
    },
    p: ({ children }) => <p className="my-4 leading-7 text-slate-700 dark:text-slate-200">{children}</p>,
    ul: ({ children }) => <ul className="my-3 list-disc space-y-1 pl-6">{children}</ul>,
    ol: ({ children }) => <ol className="my-3 list-decimal space-y-1 pl-6">{children}</ol>,
    blockquote: ({ children }) => (
      <blockquote className="my-4 border-l-4 border-slate-300 pl-4 italic text-slate-600 dark:border-slate-700 dark:text-slate-300">
        {children}
      </blockquote>
    ),
    Hint,
    Warning,
    Info,
    CodeTabs,
    AutoCodeTabs
  };
}
