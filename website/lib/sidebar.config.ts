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
    items: [
      item("Introduction", "introduction/introduction"),
      item("Overview", "introduction/overview"),
      item("First steps", "introduction/first-steps"),
      item("Controllers", "introduction/controllers"),
      item("Providers", "introduction/providers"),
      item("Modules", "introduction/modules"),
      item("Middleware", "introduction/middleware"),
      item("Exception filters", "introduction/exception-filters"),
      item("Pipes", "introduction/pipes"),
      item("Guards", "introduction/guards"),
      item("Interceptors", "introduction/interceptors"),
      item("Custom decorators", "introduction/custom-decorators")
    ]
  },
  {
    id: "fundamentals",
    title: "Fundamentals",
    items: [
      item("Fundamentals", "fundamentals/fundamentals"),
      item("Custom providers", "fundamentals/custom-providers"),
      item("Asynchronous providers", "fundamentals/asynchronous-providers"),
      item("Dynamic modules", "fundamentals/dynamic-modules"),
      item("Injection scopes", "fundamentals/injection-scopes"),
      item("Circular dependency", "fundamentals/circular-dependency"),
      item("Module reference", "fundamentals/module-reference"),
      item("Lazy-loading modules", "fundamentals/lazy-loading-modules"),
      item("Execution context", "fundamentals/execution-context"),
      item("Lifecycle events", "fundamentals/lifecycle-events"),
      item("Discovery service", "fundamentals/discovery-service"),
      item("Platform agnosticism", "fundamentals/platform-agnosticism"),
      item("Testing", "fundamentals/testing")
    ]
  },
  {
    id: "techniques",
    title: "Techniques",
    items: [
      item("Configuration", "techniques/configuration"),
      item("Database", "techniques/database"),
      item("Mongo", "techniques/mongo"),
      item("Validation", "techniques/validation"),
      item("Caching", "techniques/caching"),
      item("Serialization", "techniques/serialization"),
      item("Versioning", "techniques/versioning"),
      item("Task scheduling", "techniques/task-scheduling"),
      item("Queues", "techniques/queues"),
      item("Logging", "techniques/logging"),
      item("Cookies", "techniques/cookies"),
      item("Events", "techniques/events"),
      item("Compression", "techniques/compression"),
      item("File upload", "techniques/file-upload"),
      item("Streaming files", "techniques/streaming-files"),
      item("HTTP module", "techniques/http-module"),
      item("Session", "techniques/session"),
      item("Model-View-Controller", "techniques/model-view-controller"),
      item("Performance (Fastify)", "techniques/performance-fastify"),
      item("Server-Sent Events", "techniques/server-sent-events")
    ]
  },
  {
    id: "security",
    title: "Security",
    items: [
      item("Security", "security/security"),
      item("Authentication", "security/authentication"),
      item("Authorization", "security/authorization"),
      item("Encryption and Hashing", "security/encryption-and-hashing"),
      item("Helmet", "security/helmet"),
      item("CORS", "security/cors"),
      item("CSRF Protection", "security/csrf-protection"),
      item("Rate limiting", "security/rate-limiting")
    ]
  },
  {
    id: "graphql",
    title: "GraphQL",
    items: [
      item("Quick start", "graphql/quick-start"),
      item("Resolvers", "graphql/resolvers"),
      item("Mutations", "graphql/mutations"),
      item("Subscriptions", "graphql/subscriptions"),
      item("Scalars", "graphql/scalars"),
      item("Directives", "graphql/directives"),
      item("Interfaces", "graphql/interfaces"),
      item("Unions and Enums", "graphql/unions-and-enums"),
      item("Field middleware", "graphql/field-middleware"),
      item("Mapped types", "graphql/mapped-types"),
      item("Plugins", "graphql/plugins"),
      item("Complexity", "graphql/complexity"),
      item("Extensions", "graphql/extensions"),
      item("CLI Plugin", "graphql/cli-plugin"),
      item("Generating SDL", "graphql/generating-sdl"),
      item("Sharing models", "graphql/sharing-models"),
      item("Other features", "graphql/other-features"),
      item("Federation", "graphql/federation")
    ]
  },
  {
    id: "websockets",
    title: "WebSockets",
    items: [
      item("Gateways", "websockets/gateways"),
      item("Exception filters", "websockets/exception-filters"),
      item("Pipes", "websockets/pipes"),
      item("Guards", "websockets/guards"),
      item("Interceptors", "websockets/interceptors"),
      item("Adapters", "websockets/adapters")
    ]
  },
  {
    id: "microservices",
    title: "Microservices",
    items: [
      item("Overview", "microservices/overview"),
      item("Redis", "microservices/redis"),
      item("MQTT", "microservices/mqtt"),
      item("NATS", "microservices/nats"),
      item("RabbitMQ", "microservices/rabbitmq"),
      item("Kafka", "microservices/kafka"),
      item("gRPC", "microservices/grpc"),
      item("Custom transporters", "microservices/custom-transporters"),
      item("Exception filters", "microservices/exception-filters"),
      item("Pipes", "microservices/pipes"),
      item("Guards", "microservices/guards"),
      item("Interceptors", "microservices/interceptors"),
      item("NEWDeployment", "microservices/new-deployment")
    ]
  },
  {
    id: "deployment",
    title: "Deployment",
    items: [item("Standalone apps", "deployment/standalone-apps")]
  },
  {
    id: "cli",
    title: "CLI",
    items: [
      item("Overview", "cli/overview"),
      item("Workspaces", "cli/workspaces"),
      item("Libraries", "cli/libraries"),
      item("Usage", "cli/usage"),
      item("Scripts", "cli/scripts")
    ]
  },
  {
    id: "openapi",
    title: "OpenAPI",
    items: [
      item("Introduction", "openapi/introduction"),
      item("Types and Parameters", "openapi/types-and-parameters"),
      item("Operations", "openapi/operations"),
      item("Security", "openapi/security"),
      item("Mapped Types", "openapi/mapped-types"),
      item("Decorators", "openapi/decorators"),
      item("CLI Plugin", "openapi/cli-plugin"),
      item("Other features", "openapi/other-features")
    ]
  },
  {
    id: "recipes",
    title: "Recipes",
    items: [
      item("REPL", "recipes/repl"),
      item("CRUD generator", "recipes/crud-generator"),
      item("SWC (fast compiler)", "recipes/swc-fast-compiler"),
      item("Passport (auth)", "recipes/passport-auth"),
      item("Hot reload", "recipes/hot-reload"),
      item("MikroORM", "recipes/mikroorm"),
      item("TypeORM", "recipes/typeorm"),
      item("Mongoose", "recipes/mongoose"),
      item("Sequelize", "recipes/sequelize"),
      item("Router module", "recipes/router-module"),
      item("Swagger", "recipes/swagger"),
      item("Health checks", "recipes/health-checks"),
      item("CQRS", "recipes/cqrs"),
      item("Compodoc", "recipes/compodoc"),
      item("Prisma", "recipes/prisma"),
      item("Sentry", "recipes/sentry"),
      item("Serve static", "recipes/serve-static"),
      item("Commander", "recipes/commander"),
      item("Async local storage", "recipes/async-local-storage"),
      item("Necord", "recipes/necord"),
      item("Suites (Automock)", "recipes/suites-automock")
    ]
  },
  {
    id: "faq",
    title: "FAQ",
    items: [
      item("Serverless", "faq/serverless"),
      item("HTTP adapter", "faq/http-adapter"),
      item("Keep-Alive connections", "faq/keep-alive-connections"),
      item("Global path prefix", "faq/global-path-prefix"),
      item("Raw body", "faq/raw-body"),
      item("Hybrid application", "faq/hybrid-application"),
      item("HTTPS & multiple servers", "faq/https-and-multiple-servers"),
      item("Request lifecycle", "faq/request-lifecycle"),
      item("Common errors", "faq/common-errors")
    ]
  },
  {
    id: "devtools",
    title: "Devtools",
    items: [
      item("Overview", "devtools/overview"),
      item("CI/CD integration", "devtools/cicd-integration"),
      item("Migration guide", "devtools/migration-guide"),
      item("API Reference", "devtools/api-reference"),
      item("Official courses", "devtools/official-courses")
    ]
  },
  {
    id: "discover",
    title: "Discover",
    items: [
      item("Who is using Nest?", "discover/who-is-using-nest"),
      item("Jobs board", "discover/jobs-board"),
      item("Support us", "discover/support-us")
    ]
  }
];

export const flatSidebarItems = sidebarSections.flatMap((section) =>
  section.items.map((entry) => ({ ...entry, sectionId: section.id, sectionTitle: section.title }))
);

export const defaultDocSlug = "introduction/overview";
