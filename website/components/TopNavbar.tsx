"use client";

import { Moon, Search, Sun } from "lucide-react";
import Link from "next/link";
import { useEffect, useState } from "react";
import { SearchModal, type SearchRecord } from "@/components/search-modal";
import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";

type TopNavbarProps = {
  searchIndex: SearchRecord[];
};

export function TopNavbar({ searchIndex }: TopNavbarProps) {
  const [dark, setDark] = useState(false);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const stored = localStorage.getItem("nestrs-docs-theme");
    const preferDark = stored ? stored === "dark" : window.matchMedia("(prefers-color-scheme: dark)").matches;
    document.documentElement.classList.toggle("dark", preferDark);
    setDark(preferDark);
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setOpen((prev) => !prev);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const toggleTheme = () => {
    const next = !dark;
    setDark(next);
    document.documentElement.classList.toggle("dark", next);
    localStorage.setItem("nestrs-docs-theme", next ? "dark" : "light");
  };

  return (
    <>
      <header className="sticky top-0 z-40 border-b border-slate-200 bg-white dark:border-slate-800/40 dark:bg-ink">
        <div className="flex h-14 items-center gap-3 px-4">
          <Link href="/" className="inline-flex items-center gap-2 text-sm font-semibold text-ink dark:text-cloud">
            <span className="h-2.5 w-2.5 rounded-[2px] bg-ember" aria-hidden="true" />
            nestrs
          </Link>

          <Button
            variant="outline"
            onClick={() => setOpen(true)}
            className="mx-auto flex h-9 w-full max-w-md items-center justify-between border-slate-200 bg-slate-50 px-3 text-left text-sm text-slate-600 hover:border-ember dark:border-slate-800/60 dark:bg-slate-900 dark:text-slate"
          >
            <span className="inline-flex items-center gap-2">
              <Search size={14} />
              Search docs
            </span>
            <kbd className="rounded border border-slate-300 px-1.5 text-xs dark:border-slate-800">CMD+K</kbd>
          </Button>

          <Select defaultValue="v0.3.3">
            <SelectTrigger className="h-9 w-[86px] border-slate-300 bg-white text-xs text-ink dark:border-slate-800 dark:bg-slate-900 dark:text-cloud">
              <SelectValue />
            </SelectTrigger>
            <SelectContent className="border-slate-200 dark:border-slate-800">
              <SelectItem value="v0.3.3">v0.3.3</SelectItem>
              <SelectItem value="v0.3.2">v0.3.2</SelectItem>
            </SelectContent>
          </Select>

          <Button
            variant="outline"
            size="icon"
            onClick={toggleTheme}
            className="h-9 w-9 border-slate-300 text-slate-700 hover:border-ember hover:text-ink dark:border-slate-800 dark:text-slate dark:hover:text-cloud"
            aria-label="Toggle color mode"
          >
            {dark ? <Sun size={16} /> : <Moon size={16} />}
          </Button>

          <a href="https://github.com/Joshyahweh/nestrs" className="text-sm text-slate-700 hover:text-ember dark:text-slate dark:hover:text-cloud">
            GitHub
          </a>
        </div>
      </header>
      <SearchModal open={open} onClose={() => setOpen(false)} records={searchIndex} />
    </>
  );
}
