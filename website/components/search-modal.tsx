"use client";

import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";

export type SearchRecord = {
  slug: string;
  title: string;
  sectionTitle: string;
  headings: { id: string; text: string }[];
};

type SearchModalProps = {
  open: boolean;
  onClose: () => void;
  records: SearchRecord[];
};

export function SearchModal({ open, onClose, records }: SearchModalProps) {
  const [query, setQuery] = useState("");

  useEffect(() => {
    if (!open) setQuery("");
  }, [open]);

  const grouped = useMemo(() => {
    const q = query.trim().toLowerCase();
    const matches = records
      .map((record) => {
        const headingMatches = record.headings.filter((h) => h.text.toLowerCase().includes(q));
        const titleMatch = record.title.toLowerCase().includes(q);
        const sectionMatch = record.sectionTitle.toLowerCase().includes(q);
        if (!q || titleMatch || sectionMatch || headingMatches.length > 0) {
          return {
            ...record,
            headingMatches: q ? headingMatches : record.headings.slice(0, 3)
          };
        }
        return null;
      })
      .filter(Boolean) as Array<SearchRecord & { headingMatches: { id: string; text: string }[] }>;

    return matches.reduce<Record<string, Array<SearchRecord & { headingMatches: { id: string; text: string }[] }>>>(
      (acc, entry) => {
        acc[entry.sectionTitle] ??= [];
        acc[entry.sectionTitle].push(entry);
        return acc;
      },
      {}
    );
  }, [query, records]);

  return (
    <Dialog open={open} onOpenChange={(value) => (!value ? onClose() : undefined)}>
      <DialogContent
        className="max-w-3xl gap-2 border-slate-200 bg-popover p-0 text-popover-foreground dark:border-slate-800/60"
        showCloseButton={false}
      >
        <DialogHeader className="border-b border-slate-200 p-3 dark:border-slate-800/60">
          <DialogTitle className="sr-only">Search docs</DialogTitle>
          <Input
            autoFocus
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search page titles and headings..."
            className="h-10 border-slate-300 bg-background text-sm text-foreground placeholder:text-muted-foreground dark:border-slate-800 dark:bg-slate-900"
          />
        </DialogHeader>
        <div className="max-h-[70vh] overflow-auto p-2">
          {Object.entries(grouped).map(([section, entries]) => (
            <div key={section} className="mb-4">
              <p className="px-2 pb-1 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">{section}</p>
              {entries.map((entry) => (
                <div
                  key={entry.slug}
                  className="mb-1 rounded-md border border-transparent p-2 hover:border-slate-300 hover:bg-slate-100 dark:hover:border-slate-800 dark:hover:bg-slate-800/40"
                >
                  <Link className="block text-sm font-medium text-foreground" href={`/docs/${entry.slug}`} onClick={onClose}>
                    {entry.title}
                  </Link>
                  <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1">
                    {entry.headingMatches.slice(0, 3).map((heading) => (
                      <Link
                        key={`${entry.slug}-${heading.id}`}
                        href={`/docs/${entry.slug}#${heading.id}`}
                        className="text-xs text-muted-foreground hover:text-ember"
                        onClick={onClose}
                      >
                        #{heading.text}
                      </Link>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}
