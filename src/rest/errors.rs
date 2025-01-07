use std::fmt;
use std::fmt::Formatter;

use actix_web::{error::JsonPayloadError, HttpResponse, ResponseError};
use serde::Serialize;

#[derive(Debug)]
pub enum ApiError {
    Generic(codes::ResultCode, &'static str, Option<String>),
    Validation(String, Option<String>),
    Auth(&'static str, Option<String>),
    NotFound,
}

impl std::error::Error for ApiError {}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
impl std::convert::From<ApiError> for HttpResponse {
    fn from(error: ApiError) -> Self {
        ApiErrorData::from(error).into()
    }
}

impl std::convert::From<ApiError> for ApiErrorData {
    fn from(error: ApiError) -> Self {
        match error {
            ApiError::NotFound => ApiErrorData {
                code: codes::ResultCode::NotFound,
                message: codes::NOT_FOUND.to_string(),
                reason: Some("resource not found".to_string()),
            },
            ApiError::Generic(code, msg, ctx) => ApiErrorData {
                code,
                message: msg.to_string(),
                reason: ctx,
            },
            ApiError::Auth(msg, ctx) => ApiErrorData {
                code: codes::ResultCode::Unauthorized,
                message: msg.to_string(),
                reason: ctx,
            },
            ApiError::Validation(msg, ctx) => ApiErrorData {
                code: codes::ResultCode::BadRequest,
                message: msg.to_string(),
                reason: ctx,
            },
        }
    }
}

#[derive(Default, Debug, Clone, Serialize)]
pub struct ApiErrorData {
    pub code: codes::ResultCode,
    pub message: String,
    pub reason: Option<String>,
}

impl std::fmt::Display for ApiErrorData {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}: {}; {:?})", self.code, self.message, self.reason)
    }
}

impl std::error::Error for ApiErrorData {}

impl std::convert::From<JsonPayloadError> for ApiErrorData {
    fn from(error: JsonPayloadError) -> Self {
        ApiError::Generic(
            codes::ResultCode::BadRequest,
            codes::INVALID_PAYLOAD,
            Some(error.to_string()),
        )
        .into()
    }
}

impl ResponseError for ApiErrorData {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::from(self.clone())
    }
}

#[derive(Default, Debug, Clone, Serialize)]
pub struct ApiOk {
    pub code: String,
}

impl std::convert::From<ApiOk> for HttpResponse {
    fn from(ok: ApiOk) -> Self {
        HttpResponse::Ok().json(ok)
    }
}

pub fn ok_result() -> ApiOk {
    ApiOk {
        code: codes::ResultCode::Ok.to_string(),
    }
}

pub fn bad_request(msg: &str, reason: Option<String>) -> HttpResponse {
    ApiError::Validation(msg.to_string(), reason).into()
}

pub fn internal_error(description: &str) -> HttpResponse {
    ApiError::Generic(
        codes::ResultCode::ServerError,
        codes::INTERNAL_ERROR,
        Some(description.to_string()),
    )
    .into()
}

impl std::convert::From<ApiErrorData> for HttpResponse {
    fn from(error: ApiErrorData) -> Self {
        #[derive(Serialize)]
        struct Response {
            pub error: ApiErrorData,
        }

        let mut resp = HttpResponse::build(error.code.clone().into());
        resp.json(&Response { error })
    }
}

pub mod codes {
    use actix_web::http::StatusCode;
    use serde::de::Visitor;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub const INTERNAL_ERROR: &str = "INTERNAL_SERVER_ERROR";
    pub const NOT_FOUND: &str = "NOT_FOUND";
    pub const INVALID_PAYLOAD: &str = "INVALID_PAYLOAD";

    #[derive(Clone, Debug)]
    pub enum ResultCode {
        Ok,                  // - success
        BadRequest,          // - something is wrong with the data that was sent
        Unauthorized,        // - bad or lack of Authorization
        Forbidden,           // - no access to data, for example by policies
        NotFound,            // - standard - no route
        UnprocessableEntity, // -
        ServerError,         // - error on the server that the client cannot fix
        Other(u16),
    }

    impl std::fmt::Display for ResultCode {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let status_code: StatusCode = self.into();
            status_code.fmt(f)
        }
    }
    impl Default for ResultCode {
        fn default() -> Self {
            ResultCode::Other(0)
        }
    }

    impl std::convert::From<u16> for ResultCode {
        fn from(code: u16) -> Self {
            match code {
                200 => ResultCode::Ok,
                400 => ResultCode::BadRequest,
                401 => ResultCode::Unauthorized,
                403 => ResultCode::Forbidden,
                404 => ResultCode::NotFound,
                422 => ResultCode::UnprocessableEntity,
                500 => ResultCode::ServerError,
                _ => ResultCode::Other(code),
            }
        }
    }

    impl std::convert::From<ResultCode> for StatusCode {
        fn from(code: ResultCode) -> StatusCode {
            StatusCode::from_u16(code.into()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }

    impl std::convert::From<&ResultCode> for StatusCode {
        fn from(code: &ResultCode) -> StatusCode {
            StatusCode::from_u16(code.into()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }

    impl std::convert::From<ResultCode> for u16 {
        fn from(code: ResultCode) -> u16 {
            u16::from(&code)
        }
    }

    impl std::convert::From<&ResultCode> for u16 {
        fn from(code: &ResultCode) -> u16 {
            match code {
                ResultCode::Ok => 200,
                ResultCode::BadRequest => 400,
                ResultCode::Unauthorized => 401,
                ResultCode::Forbidden => 403,
                ResultCode::NotFound => 404,
                ResultCode::UnprocessableEntity => 422,
                ResultCode::ServerError => 500,
                ResultCode::Other(code) => *code,
            }
        }
    }

    impl Serialize for ResultCode {
        fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_u16(self.into())
        }
    }

    impl<'de> Deserialize<'de> for ResultCode {
        fn deserialize<D>(deserializer: D) -> std::result::Result<ResultCode, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct ResultCodeVisitor;
            impl<'de> Visitor<'de> for ResultCodeVisitor {
                type Value = ResultCode;

                fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    formatter.write_str("an number must be between 100 and 600")
                }

                fn visit_u16<E>(self, v: u16) -> std::result::Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    if !(100..600).contains(&v) {
                        return Err(E::custom("an number must be between 100 and 600"));
                    }
                    Ok(Self::Value::from(v))
                }
            }

            deserializer.deserialize_i32(ResultCodeVisitor)
        }
    }
}
