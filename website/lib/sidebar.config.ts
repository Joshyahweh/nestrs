/**
 * Website docs navigation — mirrors `docs/src/SUMMARY.md` (mdBook).
 * Only pages backed by markdown in `docs/src/` appear here (no NestJS placeholder tree).
 */
export type SidebarItem = {
  title: string;
  slug: string;
};

export type SidebarSection = {
  id: string;
  title: string;
  items: SidebarItem[];
};

const item = (title: string, slug: string): SidebarItem => ({ title, slug });

export const sidebarSections: SidebarSection[] = [
  {
    id: "introduction",
    title: "Introduction",
    items: [item("Introduction", "introduction")]
  },
  {
    id: "getting-started",
    title: "Getting started",
    items: [
      item("First steps", "first-steps"),
      item("Backend stack recipes", "backend-recipes"),
      item("NestJS → nestrs migration", "nestjs-migration"),
      item("CLI (nestrs-scaffold)", "cli")
    ]
  },
  {
    id: "core",
    title: "Core",
    items: [
      item("Custom decorators (Nest → Rust)", "custom-decorators"),
      item("Fundamentals (DI, scopes, lifecycle)", "fundamentals"),
      item("Observability", "observability"),
      item("Ecosystem modules", "ecosystem"),
      item("Microservices", "microservices"),
      item("GraphQL, WebSockets & microservices DX", "graphql-ws-micro-dx"),
      item("Production runbook", "production"),
      item("OpenAPI & HTTP (schemas, security)", "openapi-http")
    ]
  },
  {
    id: "security",
    title: "Security & pipeline",
    items: [
      item("Security", "security"),
      item("Secure defaults checklist", "secure-defaults"),
      item("HTTP pipeline order", "http-pipeline-order")
    ]
  },
  {
    id: "project",
    title: "Project",
    items: [
      item("Contributing", "contributing"),
      item("Architecture decisions (ADRs)", "adrs"),
      item("Release", "release"),
      item("Changelog", "changelog"),
      item("Roadmap parity", "roadmap-parity"),
      item("API cookbook (`NestApplication` & CLI)", "appendix-api-cookbook")
    ]
  }
];

export const flatSidebarItems = sidebarSections.flatMap((section) =>
  section.items.map((entry) => ({ ...entry, sectionId: section.id, sectionTitle: section.title }))
);

/** Default route: `docs/src/index.md` */
export const defaultDocSlug = "introduction";
