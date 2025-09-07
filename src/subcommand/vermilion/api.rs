use std::borrow::Cow;

use aide::{
  axum::IntoApiResponse, generate::GenContext, openapi::{MediaType, OpenApi, Operation}, OperationOutput
};
use axum::{
  http::StatusCode,
  response::{IntoResponse, Response},
  Extension, Json,
};
use indexmap::IndexMap;
use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};

use crate::InscriptionId;

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
                json_schema: json_schema!({
                  "type": "string",
                  "description": "Bad request error message",
                  "example": "Invalid parameter value"
                }),
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
                json_schema: json_schema!({
                  "type": "string",
                  "description": "Resource not found error message",
                  "example": "Resource not found"
                }),
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
                json_schema: json_schema!({
                  "type": "string",
                  "description": "Internal server error message",
                  "example": "Internal server error occurred"
                }),
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

impl JsonSchema for InscriptionId {
  fn schema_name() -> Cow<'static, str> {
    "InscriptionId".into()
  }
  // Note: path parameters are expected in object properties
  fn json_schema(_gen: &mut SchemaGenerator) -> Schema {
    json_schema!({
      "type": "object",
      "properties": {
        "inscription_id": {
          "type": "string",
          "pattern": "^[0-9a-fA-F]{64}i\\d+$",
          "description": "Inscription ID: 64 hex characters followed by 'i' and a number",
          "example": "6fb976ab49dcec017f1e201e84395983204ae1a7c2abf7ced0a85d692e442799i0"
        }
      },
      "required": ["inscription_id"]
    })
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TxidParam(pub bitcoin::Txid);

impl JsonSchema for TxidParam {
  fn schema_name() -> std::borrow::Cow<'static, str> {
    "Txid".into()
  }

  fn json_schema(_gen: &mut SchemaGenerator) -> Schema {
    json_schema!({
      "type": "object",
      "properties": {
        "txid": {
          "type": "string",
          "pattern": "^[0-9a-fA-F]{64}$",
          "description": "Transaction ID: 64 hex characters",
          "example": "6fb976ab49dcec017f1e201e84395983204ae1a7c2abf7ced0a85d692e442799"
        }
      },
      "required": ["txid"]
    })
  }
}