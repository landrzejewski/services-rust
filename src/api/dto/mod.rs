//! Data Transfer Objects – the public JSON contract of the API.
//!
//! - `*Request` / `*Query` – input: `Deserialize`, converted INTO domain types,
//! - `*Response`           – output: `Serialize`, converted FROM domain types.
//!
//! Conversions use the standard traits:
//! - `From<A> for B`    – infallible mapping; gives `a.into()` for free,
//! - `TryFrom<A> for B` – mapping that can fail (`Result`); gives `a.try_into()` (used in step 009).
//!
//! Why separate types (instead of `#[derive(Serialize)]` on domain models):
//! - the API contract changes only on purpose (renaming a domain field doesn't break clients),
//! - internal fields can't leak by accident (e.g. password hashes in step 018),
//! - output can be shaped for clients (derived fields, flattened data, links),
//! - input contains only what clients may set (no `id`, `status`, `createdAt` in requests).

pub mod bookings;
pub mod pagination;
pub mod rooms;
pub mod users;
