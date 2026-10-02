//! Offset-based pagination (step 015).

use crate::domain::validation::InvalidValue;

/// Which page the caller wants. `page` is 1-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRequest {
    page: u32,
    size: u32,
}

impl PageRequest {
    pub const MAX_SIZE: u32 = 100;

    pub fn new(page: u32, size: u32) -> Result<Self, InvalidValue> {
        if page == 0 {
            return Err(InvalidValue::new("page", "must be at least 1"));
        }
        if size == 0 || size > Self::MAX_SIZE {
            return Err(InvalidValue::new(
                "size",
                format!("must be between 1 and {}", Self::MAX_SIZE),
            ));
        }
        Ok(Self { page, size })
    }

    pub fn page(&self) -> u32 {
        self.page
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    /// Number of rows to skip – SQL `OFFSET`.
    pub fn offset(&self) -> u64 {
        u64::from(self.page - 1) * u64::from(self.size)
    }
}

impl Default for PageRequest {
    fn default() -> Self {
        Self { page: 1, size: 20 }
    }
}

/// One page of results plus the information needed to navigate.
#[derive(Debug, Clone)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub request: PageRequest,
    /// Number of all matching rows (all pages).
    pub total: u64,
}

impl<T> Page<T> {
    pub fn total_pages(&self) -> u64 {
        self.total.div_ceil(u64::from(self.request.size()))
    }

    /// Converts the items, keeping paging information (e.g. domain -> DTO).
    pub fn map<U>(self, f: impl FnMut(T) -> U) -> Page<U> {
        Page {
            items: self.items.into_iter().map(f).collect(),
            request: self.request,
            total: self.total,
        }
    }
}
