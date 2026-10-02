// Diesel schema – normally generated: `diesel print-schema > src/.../schema.rs`.
// Describes the table for the query builder; queries are type-checked against it at compile time.
// Only the columns used by the application are listed (`created_at` is omitted).

diesel::table! {
    rooms (id) {
        id -> Uuid,
        name -> Text,
        description -> Nullable<Text>,
        capacity -> Int4,
        opens_at -> Time,
        closes_at -> Time,
    }
}
