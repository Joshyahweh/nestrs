"use client";

import { useState } from "react";
import { CodeBlock } from "@/components/mdx/CodeBlock";
import { Button } from "@/components/ui/button";

type CodeTabsProps = {
  rustCode: string;
  tsCode: string;
  rustFilename: string;
  tsFilename: string;
};

export function CodeTabs({ rustCode, tsCode, rustFilename, tsFilename }: CodeTabsProps) {
  const [tab, setTab] = useState<"rust" | "ts">("rust");

  return (
    <section className="my-6 overflow-hidden rounded-lg border border-slate-800/60 bg-code">
      <div className="flex items-center gap-2 border-b border-slate-800/60 px-3 py-2">
        <Button
          size="sm"
          variant={tab === "rust" ? "default" : "ghost"}
          className={`h-7 px-2 text-xs ${tab === "rust" ? "bg-ember text-white hover:bg-[#d23b18]" : "text-slate hover:text-cloud"}`}
          onClick={() => setTab("rust")}
        >
          Rust
        </Button>
        <Button
          size="sm"
          variant={tab === "ts" ? "default" : "ghost"}
          className={`h-7 px-2 text-xs ${tab === "ts" ? "bg-ember text-white hover:bg-[#d23b18]" : "text-slate hover:text-cloud"}`}
          onClick={() => setTab("ts")}
        >
          TypeScript
        </Button>
      </div>
      {tab === "rust" ? (
        <CodeBlock code={rustCode} language="rust" filename={rustFilename} />
      ) : (
        <CodeBlock code={tsCode} language="ts" filename={tsFilename} />
      )}
    </section>
  );
}
