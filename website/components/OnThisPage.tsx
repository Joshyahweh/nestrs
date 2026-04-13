import Link from "next/link";
import type { DocHeading } from "@/lib/docs";

type OnThisPageProps = {
  headings: DocHeading[];
};

export function OnThisPage({ headings }: OnThisPageProps) {
  if (headings.length < 4) return null;

  return (
    <aside className="sticky top-20 hidden h-[calc(100vh-80px)] w-[200px] shrink-0 overflow-y-auto pl-4 lg:block">
      <p className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate">On this page</p>
      <ul className="space-y-1">
        {headings
          .filter((heading) => heading.level === 2)
          .map((heading) => (
            <li key={heading.id}>
              <Link href={`#${heading.id}`} className="text-sm text-slate hover:text-ember">
                {heading.text}
              </Link>
            </li>
          ))}
      </ul>
    </aside>
  );
}
