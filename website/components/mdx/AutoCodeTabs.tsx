import { CodeTabs } from "@/components/mdx/CodeTabs";

type AutoCodeTabsProps = {
  section: string;
  topic: string;
  title: string;
};

const toSnake = (value: string) =>
  value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "") || "example";

const toKebab = (value: string) =>
  value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "") || "example";

const toPascal = (value: string) =>
  value
    .split(/[^a-zA-Z0-9]+/)
    .filter(Boolean)
    .map((chunk) => chunk[0].toUpperCase() + chunk.slice(1))
    .join("") || "Example";

const sampleCode = (section: string, topic: string, title: string) => {
  const snake = toSnake(topic);
  const kebab = toKebab(topic);
  const pascal = toPascal(topic);
  const dto = `${pascal}Dto`;

  switch (section) {
    case "microservices":
      return {
        rust: `use nestrs_microservices::prelude::*;

pub struct ${pascal}Handler;

#[message_pattern("${kebab}.sync")]
async fn handle(payload: ${dto}) -> Result<${dto}, MicroserviceError> {
    Ok(payload)
}`,
        ts: `@MessagePattern('${kebab}.sync')
handle(payload: ${dto}): ${dto} {
  return payload;
}`
      };
    case "websockets":
      return {
        rust: `use nestrs_ws::prelude::*;

#[gateway(namespace = "/${kebab}")]
pub struct ${pascal}Gateway;

#[subscribe("${kebab}:join")]
async fn on_join(client: WsClient) -> WsResult {
    client.emit("joined", "${title}");
    Ok(())
}`,
        ts: `@WebSocketGateway({ namespace: '/${kebab}' })
export class ${pascal}Gateway {
  @SubscribeMessage('${kebab}:join')
  onJoin(@ConnectedSocket() client: Socket) {
    client.emit('joined', '${title}');
  }
}`
      };
    case "graphql":
      return {
        rust: `pub struct ${pascal}Resolver;

impl ${pascal}Resolver {
    pub async fn ${snake}(&self) -> Vec<${dto}> {
        vec![${dto}::new("${title}")]
    }
}`,
        ts: `@Resolver(() => ${dto})
export class ${pascal}Resolver {
  @Query(() => [${dto}])
  async ${snake}(): Promise<${dto}[]> {
    return [new ${dto}('${title}')];
  }
}`
      };
    case "security":
      return {
        rust: `use nestrs::execution_context::ExecutionContext;
use nestrs::guards::CanActivate;

pub struct ${pascal}Guard;

impl CanActivate for ${pascal}Guard {
    fn can_activate(&self, ctx: &ExecutionContext) -> bool {
        ctx.request().headers().contains_key("authorization")
    }
}`,
        ts: `@Injectable()
export class ${pascal}Guard implements CanActivate {
  canActivate(context: ExecutionContext): boolean {
    const req = context.switchToHttp().getRequest();
    return Boolean(req.headers.authorization);
  }
}`
      };
    case "openapi":
      return {
        rust: `use utoipa::ToSchema;

#[derive(ToSchema, serde::Serialize)]
pub struct ${dto} {
    pub label: String
}

pub fn ${snake}_schema() -> &'static str {
    "${title} schema registered"
}`,
        ts: `export class ${dto} {
  @ApiProperty()
  label!: string;
}

@ApiOkResponse({ type: ${dto} })`
      };
    case "techniques":
      return {
        rust: `pub struct ${pascal}Service;

impl ${pascal}Service {
    pub async fn apply(&self) -> anyhow::Result<()> {
        tracing::info!("applying ${title}");
        Ok(())
    }
}`,
        ts: `@Injectable()
export class ${pascal}Service {
  async apply(): Promise<void> {
    this.logger.log('applying ${title}');
  }
}`
      };
    case "deployment":
      return {
        rust: `use nestrs::factory::NestFactory;

#[tokio::main]
async fn main() {
    NestFactory::create::<AppModule>().listen(3000).await;
}`,
        ts: `async function bootstrap() {
  const app = await NestFactory.create(AppModule);
  await app.listen(3000);
}
bootstrap();`
      };
    case "cli":
      return {
        rust: `fn main() {
    println!("nestrs-cli ${snake} --help");
}`,
        ts: `program
  .command('${kebab}')
  .description('${title} workflow')
  .action(() => console.log('${title}'));`
      };
    case "recipes":
      return {
        rust: `pub async fn ${snake}_recipe() -> anyhow::Result<()> {
    tracing::info!("recipe: ${title}");
    Ok(())
}`,
        ts: `export async function ${snake}Recipe(): Promise<void> {
  console.log('recipe: ${title}');
}`
      };
    case "faq":
      return {
        rust: `pub fn explain_${snake}_issue() -> &'static str {
    "Check environment configuration and transport wiring."
}`,
        ts: `export function explain${pascal}Issue(): string {
  return 'Check environment configuration and adapter wiring.';
}`
      };
    case "devtools":
      return {
        rust: `pub fn emit_${snake}_metric(value: u64) {
    tracing::info!(metric = "${kebab}", value, "devtools metric");
}`,
        ts: `export function emit${pascal}Metric(value: number) {
  console.log({ metric: '${kebab}', value });
}`
      };
    case "discover":
      return {
        rust: `pub fn ${snake}_resource_url() -> &'static str {
    "https://github.com/Joshyahweh/nestrs"
}`,
        ts: `export const ${snake}ResourceUrl = 'https://github.com/Joshyahweh/nestrs';`
      };
    default:
      return {
        rust: `use nestrs::prelude::*;

#[controller("/${kebab}")]
pub struct ${pascal}Controller;

#[get("")]
async fn ${snake}() -> Json<${dto}> {
    Json(${dto}::new("${title}"))
}`,
        ts: `@Controller('${kebab}')
export class ${pascal}Controller {
  @Get()
  ${snake}(): ${dto} {
    return new ${dto}('${title}');
  }
}`
      };
  }
};

export function AutoCodeTabs({ section, topic, title }: AutoCodeTabsProps) {
  const snake = toSnake(topic);
  const { rust, ts } = sampleCode(section, topic, title);

  return (
    <CodeTabs
      rustCode={rust}
      tsCode={ts}
      rustFilename={`${snake}.rs`}
      tsFilename={`${snake}.ts`}
    />
  );
}
