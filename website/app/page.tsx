import Link from "next/link";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { CodePanel } from "@/components/ui/code-panel";

export default function HomePage() {
  return (
    <div className="bg-ink text-cloud">
      <header className="fixed inset-x-0 top-0 z-40 border-b border-slate-800/60 bg-ink">
        <div className="mx-auto flex h-16 w-full max-w-[1200px] items-center justify-between px-4">
          <Link href="/" className="inline-flex items-center gap-2 text-sm font-semibold text-white">
            <span className="h-2.5 w-2.5 rounded-[2px] bg-ember" aria-hidden="true" />
            nestrs
          </Link>
          <nav className="flex items-center gap-3 text-sm text-slate">
            <Link href="/docs/introduction" className="hover:text-ember">
              Docs
            </Link>
            <a href="https://github.com/Joshyahweh/nestrs" className="inline-flex items-center gap-2 hover:text-ember">
              GitHub
              <img
                src="https://img.shields.io/github/stars/Joshyahweh/nestrs?style=flat&label=%E2%98%85&labelColor=0F172A&color=E8411A"
                alt="GitHub stars"
                className="h-5 rounded"
              />
            </a>
            <a href="https://crates.io/crates/nestrs" className="hover:text-ember">
              <img
                src="https://img.shields.io/crates/v/nestrs?style=flat&label=crates.io&labelColor=0F172A&color=E8411A"
                alt="crates.io badge"
                className="h-5 rounded"
              />
            </a>
          </nav>
        </div>
      </header>

      <main className="pt-16">
        <section className="bg-ink py-20">
          <div className="mx-auto grid w-full max-w-[1200px] gap-8 px-4 md:grid-cols-2 md:items-start">
            <div>
              <h1 className="text-4xl font-semibold tracking-tight text-white md:text-5xl">NestJS architecture. Rust performance.</h1>
              <p className="mt-4 max-w-xl text-[15px] leading-7 text-[#64748B]">
                Modules, controllers, guards, pipes - the patterns you know, at the speed of Rust.
              </p>
              <div className="mt-8 flex flex-wrap gap-3">
                <Button asChild className="h-10 bg-ember px-5 text-sm font-medium text-white hover:bg-[#d23b18]">
                  <Link href="/docs/first-steps">Get started</Link>
                </Button>
                <Button
                  asChild
                  variant="outline"
                  className="h-10 border-slate-700 bg-transparent px-5 text-sm font-medium text-cloud hover:border-ember hover:text-ember"
                >
                  <a href="https://github.com/Joshyahweh/nestrs">View on GitHub</a>
                </Button>
              </div>
            </div>

            <CodePanel
              title="hello_controller.rs"
              language="rust"
              code={`use nestrs::prelude::*;

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/api")]
struct HelloController;

#[routes(state = AppState)]
impl HelloController {
    #[get("/hello")]
    async fn hello() -> &'static str {
        "Hello from nestrs"
    }
}

#[module(controllers = [HelloController], providers = [AppState])]
struct AppModule;`}
            />
          </div>
        </section>

        <section className="bg-cloud py-20 text-ink">
          <div className="mx-auto grid w-full max-w-[1200px] gap-4 px-4 md:grid-cols-3">
            {[
              ["Familiar syntax", "Nest-style architecture and decorators translated into explicit Rust types."],
              ["Zero-cost abstractions", "Ergonomic APIs that compile down to efficient runtime behavior."],
              ["Production ready", "Built for observability, throughput, and dependable operations."]
            ].map(([title, body]) => (
              <Card key={title} className="gap-0 border border-slate-200 py-0 ring-0">
                <CardContent className="p-5">
                  <span className="inline-block h-3 w-3 rounded-sm border-2 border-ember" aria-hidden="true" />
                  <h2 className="mt-3 text-lg font-semibold">{title}</h2>
                  <p className="mt-2 text-[15px] text-[#64748B]">{body}</p>
                </CardContent>
              </Card>
            ))}
          </div>
        </section>

        <section className="bg-cloud py-20 text-ink">
          <div className="mx-auto w-full max-w-[1200px] px-4">
            <h2 className="text-2xl font-semibold">Same mental model, different runtime ceiling</h2>
            <div className="mt-6 grid gap-4 md:grid-cols-2">
              <CodePanel
                title="NestJS (TypeScript)"
                language="ts"
                code={`@Controller("users")
export class UsersController {
  @Get(":id")
  findOne(@Param("id") id: string) {
    return { id, name: "Taylor" };
  }
}`}
              />
              <CodePanel
                title="nestrs (Rust)"
                language="rust"
                code={`use nestrs::prelude::*;

#[derive(Default)]
#[injectable]
struct AppState;

#[dto]
struct IdParam {
    id: String,
}

#[controller(prefix = "/users", version = "v1")]
struct UsersController;

#[routes(state = AppState)]
impl UsersController {
    #[get("/:id")]
    async fn find_one(#[param::param] p: IdParam) -> axum::Json<serde_json::Value> {
        axum::Json(serde_json::json!({ "id": p.id, "name": "Taylor" }))
    }
}`}
              />
            </div>
          </div>
        </section>

        <section className="bg-cloud py-20 text-ink">
          <div className="mx-auto w-full max-w-[1200px] px-4">
            <h2 className="text-2xl font-semibold">Core framework primitives</h2>
            <div className="mt-6 grid gap-4 md:grid-cols-3">
              {[
                ["Modules", "Define boundaries for providers, imports, and exports. Keep systems composable as applications grow."],
                ["Controllers", "Map routes with macro-driven handlers that stay close to Axum semantics. Keep HTTP code explicit and readable."],
                ["Guards", "Enforce authorization and access policy before handlers execute. Reuse across transports consistently."],
                ["Pipes", "Centralize input validation and transformation. Keep business handlers focused on core logic."],
                ["Interceptors", "Attach tracing, metrics, caching, and response shaping with deterministic execution order."],
                ["Exception Filters", "Translate internal errors into stable API responses with clear contracts."]
              ].map(([title, body]) => (
                <Card key={title} className="gap-0 border border-slate-200 border-l-2 border-l-transparent bg-white py-0 ring-0 transition hover:border-l-ember">
                  <CardHeader className="p-5 pb-2">
                    <span className="inline-block h-3 w-3 rounded-sm border-2 border-ember" aria-hidden="true" />
                    <CardTitle className="mt-2 text-lg font-semibold">{title}</CardTitle>
                  </CardHeader>
                  <CardContent className="p-5 pt-0">
                    <p className="text-[15px] text-[#64748B]">{body}</p>
                  </CardContent>
                </Card>
              ))}
            </div>
          </div>
        </section>

        <section className="bg-ink py-20">
          <div className="mx-auto w-full max-w-[1200px] px-4">
            <h2 className="text-2xl font-semibold text-white">Quick install</h2>
            <p className="mt-4 inline-flex rounded border border-slate-700 px-3 py-1 font-mono text-[13.5px] text-cloud">cargo add nestrs</p>
            <div className="mt-4">
              <CodePanel
                title="main.rs"
                language="rust"
                code={`use nestrs::prelude::*;

#[tokio::main]
async fn main() {
    NestFactory::create::<AppModule>()
        .listen_graceful(3000)
        .await;
}`}
              />
            </div>
          </div>
        </section>
      </main>

      <footer className="border-t border-slate-800/60 bg-ink py-8">
        <div className="mx-auto grid w-full max-w-[1200px] items-center gap-3 px-4 text-sm text-slate md:grid-cols-3">
          <div className="inline-flex items-center gap-2 text-white">
            <span className="h-2.5 w-2.5 rounded-[2px] bg-ember" aria-hidden="true" />
            nestrs
            <span className="text-slate">MIT OR Apache-2.0</span>
          </div>
          <nav className="flex flex-wrap items-center justify-center gap-4">
            <Link href="/docs/introduction" className="hover:text-ember">
              Docs
            </Link>
            <a href="https://github.com/Joshyahweh/nestrs" className="hover:text-ember">
              GitHub
            </a>
            <a href="https://crates.io/crates/nestrs" className="hover:text-ember">
              crates.io
            </a>
            <a href="https://github.com/Joshyahweh/nestrs/blob/main/CHANGELOG.md" className="hover:text-ember">
              Changelog
            </a>
          </nav>
          <p className="text-right text-slate md:text-right">Built with Rust</p>
        </div>
      </footer>
    </div>
  );
}
