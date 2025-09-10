use std::borrow::Cow;

use aide::{
  axum::IntoApiResponse, generate::GenContext, openapi::{MediaType, OpenApi, Operation}, scalar::Scalar, OperationOutput
};
use axum::{
  http::StatusCode,
  response::{IntoResponse, Response},
  Extension, Json,
  http::HeaderMap,
};
use indexmap::IndexMap;
use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use strum::Display;

use crate::InscriptionId;

#[derive(Debug, Deserialize, JsonSchema, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SatributeType {
  Vintage,
  Nakamoto,
  Firsttransaction,
  Palindrome,
  Pizza,
  Block9,
  Block9_450,
  Block78,
  Alpha,
  Omega,
  UniformPalinception,
  PerfectPalinception,
  Block286,
  Jpeg,
  Uncommon,
  Rare,
  Epic,
  Legendary,
  Mythic,
  BlackUncommon,
  BlackRare,
  BlackEpic,
  BlackLegendary,
}


#[derive(Debug, Deserialize, JsonSchema, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum CharmType {
  Coin,
  Cursed,
  Epic,
  Legendary,
  Lost,
  Nineball,
  Rare,
  Reinscription,
  Unbound,
  Uncommon,
  Vindicated,
}


#[derive(Debug, Deserialize, JsonSchema, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ContentType {
  Text,
  Image,
  Gif,
  Audio,
  Video,
  Html,
  Json,
  Namespace,
}


#[derive(Debug, Deserialize, JsonSchema, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum InscriptionSortBy {
  Newest,
  Oldest,
  NewestSat,
  OldestSat,
  RarestSat,
  CommonestSat,
  Biggest,
  Smallest,
  HighestFee,
  LowestFee,
}


#[derive(Debug, Deserialize, JsonSchema, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum CollectionSortBy {
  BiggestOnChainFootprint,
  SmallestOnChainFootprint,
  MostVolume,
  LeastVolume,
  BiggestFileSize,
  SmallestFileSize,
  BiggestCreationFee,
  SmallestCreationFee,
  EarliestFirstInscribedDate,
  LatestFirstInscribedDate,
  EarliestLastInscribedDate,
  LatestLastInscribedDate,
  BiggestSupply,
  SmallestSupply,
}


#[derive(Debug, Deserialize, JsonSchema, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum BlockSortBy {
  Newest,
  Oldest,
  MostTxs,
  LeastTxs,
  MostInscriptions,
  LeastInscriptions,
  BiggestBlock,
  SmallestBlock,
  BiggestTotalInscriptionsSize,
  SmallestTotalInscriptionsSize,
  HighestTotalFees,
  LowestTotalFees,
  HighestInscriptionFees,
  LowestInscriptionFees,
  MostVolume,
  LeastVolume,
}


pub async fn serve_openapi(Extension(api): Extension<OpenApi>) -> impl IntoApiResponse {
  Json(api)
}

pub async fn serve_scalar(headers: HeaderMap) -> impl IntoApiResponse {
  let uri = headers.get("X-Original-URI")
    .and_then(|v| v.to_str().ok())
    .unwrap_or("/docs");
  let spec_url = if uri.to_string().starts_with("/api/") {
    "/api/api.json"
  } else {
    "/api.json"
  };
  axum::response::Html(Scalar::new(spec_url).html())
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
      ApiError::InternalServerError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response(),
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

// Newtype wrappers for path parameters

#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InscriptionNumber(pub i64);

impl JsonSchema for InscriptionNumber {
  fn schema_name() -> Cow<'static, str> {
    "InscriptionNumber".into()
  }

  fn json_schema(_gen: &mut SchemaGenerator) -> Schema {
    json_schema!({
      "type": "object",
      "properties": {
        "number": {
          "type": "integer",
          "format": "int64",
          "description": "Inscription number identifier",
          "example": 12345
        }
      },
      "required": ["number"]
    })
  }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlockNumber(pub i64);

impl JsonSchema for BlockNumber {
  fn schema_name() -> Cow<'static, str> {
    "BlockNumber".into()
  }

  fn json_schema(_gen: &mut SchemaGenerator) -> Schema {
    json_schema!({
      "type": "object",
      "properties": {
        "block": {
          "type": "integer",
          "format": "int64",
          "description": "Bitcoin block number",
          "example": 800000
        }
      },
      "required": ["block"]
    })
  }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SatNumber(pub i64);

impl JsonSchema for SatNumber {
  fn schema_name() -> Cow<'static, str> {
    "SatNumber".into()
  }

  fn json_schema(_gen: &mut SchemaGenerator) -> Schema {
    json_schema!({
      "type": "object",
      "properties": {
        "sat": {
          "type": "integer",
          "format": "int64",
          "description": "Satoshi number identifier",
          "example": 1000000000
        }
      },
      "required": ["sat"]
    })
  }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sha256Hash(pub String);

impl JsonSchema for Sha256Hash {
  fn schema_name() -> Cow<'static, str> {
    "Sha256Hash".into()
  }

  fn json_schema(_gen: &mut SchemaGenerator) -> Schema {
    json_schema!({
      "type": "object",
      "properties": {
        "sha256": {
          "type": "string",
          "pattern": "^[0-9a-fA-F]{64}$",
          "description": "SHA256 hash: 64 hex characters",
          "example": "6fb976ab49dcec017f1e201e84395983204ae1a7c2abf7ced0a85d692e442799"
        }
      },
      "required": ["sha256"]
    })
  }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BitcoinAddress(pub String);

impl JsonSchema for BitcoinAddress {
  fn schema_name() -> Cow<'static, str> {
    "BitcoinAddress".into()
  }

  fn json_schema(_gen: &mut SchemaGenerator) -> Schema {
    json_schema!({
      "type": "object",
      "properties": {
        "address": {
          "type": "string",
          "description": "Bitcoin address",
          "example": "bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh"
        }
      },
      "required": ["address"]
    })
  }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CollectionSymbol(pub String);

impl JsonSchema for CollectionSymbol {
  fn schema_name() -> Cow<'static, str> {
    "CollectionSymbol".into()
  }

  fn json_schema(_gen: &mut SchemaGenerator) -> Schema {
    json_schema!({
      "type": "object",
      "properties": {
        "collection_symbol": {
          "type": "string",
          "description": "Collection symbol identifier",
          "example": "bitcoin-puppets"
        }
      },
      "required": ["collection_symbol"]
    })
  }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParentList(pub String);

impl JsonSchema for ParentList {
  fn schema_name() -> Cow<'static, str> {
    "ParentList".into()
  }

  fn json_schema(_gen: &mut SchemaGenerator) -> Schema {
    json_schema!({
      "type": "object",
      "properties": {
        "parents": {
          "type": "string",
          "description": "Parent inscription identifiers",
          "example": "6fb976ab49dcec017f1e201e84395983204ae1a7c2abf7ced0a85d692e442799i0"
        }
      },
      "required": ["parents"]
    })
  }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SearchQuery(pub String);

impl JsonSchema for SearchQuery {
  fn schema_name() -> Cow<'static, str> {
    "SearchQuery".into()
  }

  fn json_schema(_gen: &mut SchemaGenerator) -> Schema {
    json_schema!({
      "type": "object",
      "properties": {
        "search_query": {
          "type": "string",
          "description": "Search query string",
          "example": "bitcoin"
        }
      },
      "required": ["search_query"]
    })
  }
}