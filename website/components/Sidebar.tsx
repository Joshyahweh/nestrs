"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useMemo, useState } from "react";
import { sidebarSections } from "@/lib/sidebar.config";
import { cn } from "@/lib/utils";
import {
  Sidebar as UiSidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem
} from "@/components/ui/sidebar";

type SidebarProps = {
  currentSlug: string;
};

export function Sidebar({ currentSlug }: SidebarProps) {
  const pathname = usePathname();
  const activePath = pathname.replace(/^\/docs\//, "");

  const defaultOpen = useMemo(() => {
    const found = sidebarSections.find((section) => section.items.some((item) => item.slug === currentSlug));
    return found?.id ?? "introduction";
  }, [currentSlug]);

  const [openSections, setOpenSections] = useState<Record<string, boolean>>({
    [defaultOpen]: true
  });

  return (
    <UiSidebar
      side="left"
      collapsible="none"
      className="fixed inset-y-0 left-0 z-30 hidden h-screen border-r border-sidebar-border/60 bg-sidebar pt-[56px] md:flex"
    >
      <SidebarContent className="px-2 pb-4 pt-4">
        {sidebarSections.map((section) => {
          const isOpen = openSections[section.id] ?? section.id === defaultOpen;
          return (
            <SidebarGroup key={section.id} className="px-1 py-1">
              <SidebarGroupLabel
                className="h-auto cursor-pointer px-2 py-1 text-[11px] font-semibold uppercase tracking-wide text-sidebar-foreground/80"
                onClick={() => setOpenSections((prev) => ({ ...prev, [section.id]: !isOpen }))}
              >
                {section.title}
              </SidebarGroupLabel>
              {isOpen && (
                <SidebarGroupContent>
                  <SidebarMenu className="gap-0.5">
                    {section.items.map((entry) => {
                      const isActive = activePath === entry.slug || currentSlug === entry.slug;
                      return (
                        <SidebarMenuItem key={entry.slug}>
                          <SidebarMenuButton
                            asChild
                            isActive={isActive}
                            className={cn(
                              "h-8 rounded-r-md rounded-l-none border-l-[3px] px-2.5 text-sm transition",
                              isActive
                                ? "border-l-ember bg-sidebar-accent text-sidebar-accent-foreground"
                                : "border-l-transparent text-sidebar-foreground hover:bg-sidebar-accent/80 hover:text-sidebar-accent-foreground"
                            )}
                          >
                            <Link href={`/docs/${entry.slug}`}>{entry.title}</Link>
                          </SidebarMenuButton>
                        </SidebarMenuItem>
                      );
                    })}
                  </SidebarMenu>
                </SidebarGroupContent>
              )}
            </SidebarGroup>
          );
        })}
      </SidebarContent>
    </UiSidebar>
  );
}
