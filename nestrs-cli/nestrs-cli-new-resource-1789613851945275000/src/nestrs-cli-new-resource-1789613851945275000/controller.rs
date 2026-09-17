//! NestrsCliNewResource1789613851945275000 HTTP surface.

use nestrs::prelude::*;
use super::dto::{NestrsCliNewResource1789613851945275000, CreateNestrsCliNewResource1789613851945275000Dto, UpdateNestrsCliNewResource1789613851945275000Dto};
use super::service::NestrsCliNewResource1789613851945275000Service;

#[controller(prefix = "/nestrs-cli-new-resource-1789613851945275000")]
pub struct NestrsCliNewResource1789613851945275000Controller;

impl NestrsCliNewResource1789613851945275000Controller {
    #[get("/")]
    pub async fn list(svc: NestrsCliNewResource1789613851945275000Service) -> Json<Vec<NestrsCliNewResource1789613851945275000>> {
        Json(svc.list().await)
    }

    #[post("/")]
    #[http_code(201)]
    pub async fn create(
        ValidatedBody(input): ValidatedBody<CreateNestrsCliNewResource1789613851945275000Dto>,
        svc: NestrsCliNewResource1789613851945275000Service,
    ) -> Json<NestrsCliNewResource1789613851945275000> {
        Json(svc.create(input).await)
    }

    #[patch("/:id")]
    pub async fn update(
        PathParam(id): PathParam<String>,
        ValidatedBody(input): ValidatedBody<UpdateNestrsCliNewResource1789613851945275000Dto>,
        svc: NestrsCliNewResource1789613851945275000Service,
    ) -> Json<NestrsCliNewResource1789613851945275000> {
        let _ = (id, input);
        Json(NestrsCliNewResource1789613851945275000 { id: "".into(), name: "".into() })
    }
}
