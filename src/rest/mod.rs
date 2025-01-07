use serde::{Deserialize, Serialize};

pub mod admin_api;
pub mod api;
pub mod errors;
pub mod server;

mod api_pools;
mod context;
mod requests;
mod swagger;

#[derive(Clone, Serialize)]
pub struct ListResponseMeta {
    pub page: i32,
    pub limit: i32,
    pub offset: i32,
    pub has_more: bool,
    pub total_records: i64,
}

#[derive(Clone, Serialize)]
pub struct ListResult<T: Serialize> {
    pub meta: Option<ListResponseMeta>,
    pub records: Vec<T>,
}

impl<T: Serialize> From<Vec<T>> for ListResult<T> {
    fn from(val: Vec<T>) -> Self {
        ListResult {
            records: val,
            meta: None,
        }
    }
}

#[derive(Clone, Default, Deserialize)]
pub struct PageParams {
    pub order: Option<String>,
    pub limit: Option<i32>,
    pub page: Option<i32>,
    pub name: Option<String>,
}

impl PageParams {
    pub fn get_order(&self) -> String {
        let o = self
            .order
            .clone()
            .unwrap_or("ASC".to_string())
            .to_uppercase();

        if &o == "ASC" || &o == "DESC" {
            o
        } else {
            "ASC".to_string()
        }
    }
}
