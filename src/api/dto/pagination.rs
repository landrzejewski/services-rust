//! Pagination in the API: `?page=2&size=20` in, page envelope out.

use serde::{Deserialize, Serialize};

use crate::domain::{
    pagination::{Page, PageRequest},
    validation::InvalidValue,
};

/// Query parameters for endpoints with paging only.
#[derive(Debug, Deserialize)]
pub struct PageQuery {
    pub page: Option<u32>,
    pub size: Option<u32>,
}

impl TryFrom<PageQuery> for PageRequest {
    type Error = InvalidValue;

    fn try_from(query: PageQuery) -> Result<Self, Self::Error> {
        page_request(query.page, query.size)
    }
}

/// Shared by query DTOs that combine filters and paging.
/// (`#[serde(flatten)]` of `PageQuery` into them would be nicer, but `serde_urlencoded` +
/// `flatten` can't parse numbers – every value arrives as a string.)
pub fn page_request(page: Option<u32>, size: Option<u32>) -> Result<PageRequest, InvalidValue> {
    let default = PageRequest::default();
    PageRequest::new(
        page.unwrap_or(default.page()),
        size.unwrap_or(default.size()),
    )
}

/// Envelope returned by list endpoints:
/// `{"items": [...], "page": 1, "size": 20, "totalItems": 42, "totalPages": 3}`
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageResponse<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub size: u32,
    pub total_items: u64,
    pub total_pages: u64,
}

// Generic conversion: a page of any domain type `D` becomes a page of any DTO `T` that can be
// built from `D`. One impl serves rooms, bookings and every future list.
impl<D, T: From<D>> From<Page<D>> for PageResponse<T> {
    fn from(page: Page<D>) -> Self {
        let total_pages = page.total_pages();
        let request = page.request;
        Self {
            items: page.items.into_iter().map(T::from).collect(),
            page: request.page(),
            size: request.size(),
            total_items: page.total,
            total_pages,
        }
    }
}
