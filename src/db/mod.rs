pub mod alerts;
#[cfg(feature = "enterprise")]
pub mod audit;
pub mod analytics;
pub mod api_keys;
pub mod cache;
pub mod identities;
pub mod pool;
pub mod prompts;
pub mod providers;
pub mod rbac;
pub mod requests;
pub mod smart_routing;
pub mod users;

pub use pool::DbPool;

// ── SQLite Decimal helper ─────────────────────────────────────────────────────
//
// SQLite has no native DECIMAL type. We store monetary values as TEXT and decode
// them manually. `DecimalText` wraps `rust_decimal::Decimal` and implements the
// sqlx Encode/Decode/Type traits for Sqlite (TEXT affinity).
//
// Use it only in `#[cfg(feature = "sqlite")]` row types. Postgres builds use
// `rust_decimal::Decimal` directly (supported by `sqlx/rust_decimal` feature).

#[cfg(feature = "sqlite")]
pub(crate) mod sqlite_ext {
    use rust_decimal::Decimal;
    use sqlx::{
        encode::IsNull,
        error::BoxDynError,
        sqlite::{SqliteArgumentValue, SqliteTypeInfo, SqliteValueRef},
    };
    use std::{borrow::Cow, str::FromStr};

    /// Decimal stored as TEXT in SQLite.
    #[derive(Debug, Clone)]
    pub struct DecimalText(pub Decimal);

    impl From<Decimal> for DecimalText {
        fn from(d: Decimal) -> Self {
            Self(d)
        }
    }
    impl From<DecimalText> for Decimal {
        fn from(d: DecimalText) -> Self {
            d.0
        }
    }

    impl sqlx::Type<sqlx::Sqlite> for DecimalText {
        fn type_info() -> SqliteTypeInfo {
            <str as sqlx::Type<sqlx::Sqlite>>::type_info()
        }
    }

    impl<'r> sqlx::Decode<'r, sqlx::Sqlite> for DecimalText {
        fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
            let s = <&str as sqlx::Decode<sqlx::Sqlite>>::decode(value)?;
            Decimal::from_str(s).map(DecimalText).map_err(Into::into)
        }
    }

    impl<'q> sqlx::Encode<'q, sqlx::Sqlite> for DecimalText {
        fn encode_by_ref(&self, buf: &mut Vec<SqliteArgumentValue<'q>>) -> IsNull {
            buf.push(SqliteArgumentValue::Text(Cow::Owned(self.0.to_string())));
            IsNull::No
        }
    }
}
