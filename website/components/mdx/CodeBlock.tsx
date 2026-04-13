"use client";

import { useMemo, useState } from "react";
import { Button } from "@/components/ui/button";

type CodeBlockProps = {
  code: string;
  language: string;
  filename?: string;
};

const shellLanguage = new Set(["sh", "bash", "shell", "zsh"]);

const escapeHtml = (value: string) =>
  value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

const tokenLine = (line: string, language: string) => {
  if (shellLanguage.has(language)) return escapeHtml(line);

  const pattern =
    /(\/\/.*$)|("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')|(#\[[^\]]+\]|@[\w]+|\b(?:fn|pub|struct|async|await|return|class|export|const|let|impl|use)\b)|\b(?:String|u64|u32|i64|i32|Vec|Json|Path|Result|Promise|number|boolean)\b/g;

  let output = "";
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = pattern.exec(line)) !== null) {
    output += escapeHtml(line.slice(lastIndex, match.index));
    const token = escapeHtml(match[0]);
    if (match[1]) {
      output += `<span class="text-syntax-comment">${token}</span>`;
    } else if (match[2]) {
      output += `<span class="text-syntax-string">${token}</span>`;
    } else if (match[3]) {
      output += `<span class="text-syntax-keyword">${token}</span>`;
    } else {
      output += `<span class="text-syntax-type">${token}</span>`;
    }
    lastIndex = pattern.lastIndex;
  }

  output += escapeHtml(line.slice(lastIndex));
  return output;
};

export function CodeBlock({ code, language, filename }: CodeBlockProps) {
  const [copied, setCopied] = useState(false);
  const lines = useMemo(() => code.replace(/\n$/, "").split("\n"), [code]);

  const onCopy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    } catch {
      setCopied(false);
    }
  };

  return (
    <div className="rounded-lg border border-slate-800/60 bg-code">
      <div className="flex items-center justify-between border-b border-slate-800/60 px-3 py-2">
        <div className="text-xs text-slate">{filename ?? language}</div>
        <div className="flex items-center gap-2">
          <span className="rounded bg-slate-800/70 px-2 py-0.5 text-xs uppercase tracking-wide text-slate">{language}</span>
          <Button
            variant="outline"
            size="sm"
            className="h-7 border-slate-700/70 bg-transparent px-2 text-xs text-cloud transition hover:border-ember"
            onClick={onCopy}
          >
            {copied ? "Copied" : "Copy"}
          </Button>
        </div>
      </div>
      <pre className="overflow-x-auto p-0 text-[13.5px]">
        <code className="block font-mono">
          {lines.map((line, index) => (
            <div key={`${line}-${index}`} className="grid grid-cols-[40px_1fr]">
              <span className="select-none border-r border-slate-800/60 px-2 py-[2px] text-right text-xs text-slate">
                {index + 1}
              </span>
              <span
                className="px-3 py-[2px] text-cloud"
                dangerouslySetInnerHTML={{
                  __html: tokenLine(line, language)
                }}
              />
            </div>
          ))}
        </code>
      </pre>
    </div>
  );
}
