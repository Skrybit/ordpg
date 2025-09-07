use aide::{
  axum::IntoApiResponse,
  generate::GenContext,
  openapi::{MediaType, OpenApi, Operation},
  OperationOutput,
};
use axum::{
  http::StatusCode,
  response::{IntoResponse, Response},
  Extension, Json,
};
use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::Serialize;

pub async fn serve_openapi(Extension(api): Extension<OpenApi>) -> impl IntoApiResponse {
  Json(api)
}

#[derive(Serialize, JsonSchema)]
pub enum ApiError {
  InternalServerError(String),
  NotFound(String),
  BadRequest(String),
}

impl IntoResponse for ApiError {
  fn into_response(self) -> Response {
    match self {
      ApiError::InternalServerError(msg) => {
        (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
      }
      ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg).into_response(),
      ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
    }
  }
}

impl OperationOutput for ApiError {
  type Inner = Self;

  fn inferred_responses(
    _ctx: &mut GenContext,
    _operation: &mut Operation,
  ) -> Vec<(Option<u16>, aide::openapi::Response)> {
    vec![
      (
        Some(400),
        aide::openapi::Response {
          description: "Bad Request - Invalid input parameters".into(),
          content: IndexMap::from_iter([(
            "text/plain; charset=utf-8".into(),
            MediaType {
              schema: Some(aide::openapi::SchemaObject {
                json_schema: serde_json::json!({
                  "type": "string",
                  "description": "Bad request error message",
                  "example": "Invalid parameter value"
                })
                .try_into()
                .unwrap(),
                example: Some(serde_json::json!("Invalid parameter value")),
                external_docs: None,
              }),
              ..Default::default()
            },
          )]),
          ..Default::default()
        },
      ),
      (
        Some(404),
        aide::openapi::Response {
          description: "Not Found - Resource does not exist".into(),
          content: IndexMap::from_iter([(
            "text/plain; charset=utf-8".into(),
            MediaType {
              schema: Some(aide::openapi::SchemaObject {
                json_schema: serde_json::json!({
                  "type": "string",
                  "description": "Resource not found error message",
                  "example": "Resource not found"
                })
                .try_into()
                .unwrap(),
                example: Some(serde_json::json!("Resource not found")),
                external_docs: None,
              }),
              ..Default::default()
            },
          )]),
          ..Default::default()
        },
      ),
      (
        Some(500),
        aide::openapi::Response {
          description: "Internal Server Error - Server encountered an error".into(),
          content: IndexMap::from_iter([(
            "text/plain; charset=utf-8".into(),
            MediaType {
              schema: Some(aide::openapi::SchemaObject {
                json_schema: serde_json::json!({
                  "type": "string",
                  "description": "Internal server error message",
                  "example": "Internal server error occurred"
                })
                .try_into()
                .unwrap(),
                example: Some(serde_json::json!("Internal server error occurred")),
                external_docs: None,
              }),
              ..Default::default()
            },
          )]),
          ..Default::default()
        },
      ),
    ]
  }
}
