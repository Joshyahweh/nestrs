import fs from "node:fs";
import path from "node:path";
import matter from "gray-matter";
import { defaultDocSlug, flatSidebarItems } from "@/lib/sidebar.config";

const DOCS_DIR = path.join(process.cwd(), "docs");

export type DocHeading = {
  id: string;
  text: string;
  level: 2 | 3;
};

export type LoadedDoc = {
  slug: string;
  title: string;
  description: string;
  content: string;
  headings: DocHeading[];
  sectionTitle: string;
};

const slugify = (value: string) =>
  value
    .toLowerCase()
    .replace(/[^a-z0-9\s-]/g, "")
    .trim()
    .replace(/\s+/g, "-");

const fromSidebar = new Map(flatSidebarItems.map((item) => [item.slug, item]));

const extractHeadings = (source: string): DocHeading[] => {
  const lines = source.split("\n");
  const headings: DocHeading[] = [];

  for (const line of lines) {
    if (line.startsWith("## ")) {
      const text = line.replace(/^## /, "").trim();
      headings.push({ level: 2, text, id: slugify(text) });
    } else if (line.startsWith("### ")) {
      const text = line.replace(/^### /, "").trim();
      headings.push({ level: 3, text, id: slugify(text) });
    }
  }

  return headings;
};

const fallbackExamples: Record<
  string,
  { rustTitle: string; rustCode: string; tsTitle: string; tsCode: string; cli: string; note: string }
> = {
  fundamentals: {
    rustTitle: "module-ref.rs",
    rustCode: `use nestrs_core::module_ref::ModuleRef;

pub async fn resolve_users_service(module_ref: &ModuleRef) -> anyhow::Result<()> {
    let _service = module_ref.resolve::<UsersService>().await?;
    Ok(())
}`,
    tsTitle: "module-ref.ts",
    tsCode: `constructor(private readonly moduleRef: ModuleRef) {}

async onModuleInit() {
  const service = await this.moduleRef.resolve(UsersService);
}`,
    cli: "$ cargo test -p nestrs-core module_ref",
    note: "Prefer resolving scoped providers through ModuleRef in lifecycle hooks instead of static globals."
  },
  techniques: {
    rustTitle: "users.service.rs",
    rustCode: `use sqlx::PgPool;

pub struct UsersService {
    pool: PgPool
}

impl UsersService {
    pub async fn list(&self) -> Result<Vec<UserRow>, sqlx::Error> {
        sqlx::query_as::<_, UserRow>("select id, name from users")
            .fetch_all(&self.pool)
            .await
    }
}`,
    tsTitle: "users.service.ts",
    tsCode: `@Injectable()
export class UsersService {
  constructor(private readonly prisma: PrismaService) {}

  list() {
    return this.prisma.user.findMany();
  }
}`,
    cli: "$ cargo test -p nestrs database",
    note: "Initialize pools once at startup and inject them; avoid constructing clients per request."
  },
  security: {
    rustTitle: "auth.guard.rs",
    rustCode: `use nestrs::execution_context::ExecutionContext;
use nestrs::guards::CanActivate;

pub struct AuthGuard;

impl CanActivate for AuthGuard {
    fn can_activate(&self, ctx: &ExecutionContext) -> bool {
        ctx.request()
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.starts_with("Bearer "))
            .unwrap_or(false)
    }
}`,
    tsTitle: "auth.guard.ts",
    tsCode: `@Injectable()
export class AuthGuard implements CanActivate {
  canActivate(context: ExecutionContext): boolean {
    const req = context.switchToHttp().getRequest();
    return req.headers.authorization?.startsWith("Bearer ");
  }
}`,
    cli: "$ cargo test -p nestrs authorization_bearer",
    note: "Keep authentication and authorization guards separate to make failure paths explicit."
  },
  graphql: {
    rustTitle: "users.resolver.rs",
    rustCode: `pub struct UsersResolver;

impl UsersResolver {
    pub async fn users(&self) -> Vec<UserDto> {
        vec![UserDto::new(1, "Milo".to_string())]
    }
}`,
    tsTitle: "users.resolver.ts",
    tsCode: `@Resolver(() => User)
export class UsersResolver {
  @Query(() => [User])
  users() {
    return [{ id: 1, name: "Milo" }];
  }
}`,
    cli: "$ cargo test -p nestrs-graphql",
    note: "Keep schema evolution explicit and use generated SDL snapshots in CI for stability."
  },
  openapi: {
    rustTitle: "openapi.rs",
    rustCode: `use nestrs_openapi::OpenApiBuilder;

pub fn build_openapi() -> utoipa::openapi::OpenApi {
    OpenApiBuilder::new()
        .title("nestrs API")
        .version("0.1.0")
        .build()
}`,
    tsTitle: "swagger.ts",
    tsCode: `const document = SwaggerModule.createDocument(app, config);
SwaggerModule.setup("docs", app, document);`,
    cli: "$ cargo test -p nestrs-openapi",
    note: "When roles or guards change, update OpenAPI security declarations in the same PR."
  },
  default: {
    rustTitle: "cats.controller.rs",
    rustCode: `#[controller("/cats")]
pub struct CatsController;

#[get("")]
async fn list() -> Json<Vec<CatDto>> {
    Json(vec![CatDto::new(1, "Milo".to_string())])
}`,
    tsTitle: "cats.controller.ts",
    tsCode: `@Controller('cats')
export class CatsController {
  @Get()
  list(): CatDto[] {
    return [{ id: 1, name: 'Milo' }];
  }
}`,
    cli: "$ cargo test -p nestrs",
    note: "Prefer examples that compile with the current nestrs API so copy/paste stays reliable."
  }
};

const generatedFallback = (slug: string, title: string, sectionTitle: string) => {
  const key = slug.split("/")[0];
  const example = fallbackExamples[key] ?? fallbackExamples.default;
  return `---
title: "${title}"
description: "Reference guide for ${title} in nestrs."
---

## What this page covers

This page documents **${title}** within the **${sectionTitle}** section for nestrs.

<Info>
This topic is scaffolded to mirror the docs.nestjs.com reading experience. Add concrete examples in this markdown file at \`docs/${slug}.md\`.
</Info>

## Rust implementation

\`\`\`rust filename="${example.rustTitle}"
${example.rustCode}
\`\`\`

## NestJS comparison

\`\`\`ts filename="${example.tsTitle}"
${example.tsCode}
\`\`\`

## CLI command

\`\`\`sh filename="terminal"
${example.cli}
\`\`\`

## Notes

<Hint>
${example.note}
</Hint>
`;
};

export const resolveSlug = (segments?: string[]) => {
  if (!segments || segments.length === 0) return defaultDocSlug;
  return segments.join("/");
};

export const getAllSlugs = () => flatSidebarItems.map((entry) => entry.slug.split("/"));

export const getDoc = (segments?: string[]): LoadedDoc => {
  const slug = resolveSlug(segments);
  const entry = fromSidebar.get(slug);

  const title = entry?.title ?? "Untitled";
  const sectionTitle = entry?.sectionTitle ?? "General";
  const filePath = path.join(DOCS_DIR, `${slug}.md`);

  const raw = fs.existsSync(filePath) ? fs.readFileSync(filePath, "utf8") : generatedFallback(slug, title, sectionTitle);
  const { data, content } = matter(raw);
  const headings = extractHeadings(content);

  return {
    slug,
    title: String(data.title ?? title),
    description: String(data.description ?? `Reference guide for ${title} in nestrs.`),
    content,
    headings,
    sectionTitle
  };
};

export type SearchEntry = {
  slug: string;
  title: string;
  sectionTitle: string;
  headings: { id: string; text: string }[];
};

export const getSearchIndex = (): SearchEntry[] =>
  flatSidebarItems.map((entry) => {
    const doc = getDoc(entry.slug.split("/"));
    return {
      slug: entry.slug,
      title: entry.title,
      sectionTitle: entry.sectionTitle,
      headings: doc.headings.filter((h) => h.level === 2).map((h) => ({ id: h.id, text: h.text }))
    };
  });

export const getPrevNext = (slug: string) => {
  const index = flatSidebarItems.findIndex((entry) => entry.slug === slug);
  return {
    prev: index > 0 ? flatSidebarItems[index - 1] : null,
    next: index >= 0 && index < flatSidebarItems.length - 1 ? flatSidebarItems[index + 1] : null
  };
};
