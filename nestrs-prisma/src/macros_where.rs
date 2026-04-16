//! Type-dispatched `Where` enum fragments for `prisma_model!` (only one `__prisma_where_try_*!` expands per field).

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_try_i64 {
    ($field:ident, i64) => {
        $crate::paste::paste! {
            [< $field:camel Eq >](i64),
            [< $field:camel Ne >](i64),
            [< $field:camel Gt >](i64),
            [< $field:camel Gte >](i64),
            [< $field:camel Lt >](i64),
            [< $field:camel Lte >](i64),
            [< $field:camel In >](::std::vec::Vec<i64>),
        }
    };
    ($field:ident, $other:ty) => {};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_try_string {
    ($field:ident, String) => {
        $crate::paste::paste! {
            [< $field:camel Eq >](::std::string::String),
            [< $field:camel Ne >](::std::string::String),
            [< $field:camel Gt >](::std::string::String),
            [< $field:camel Gte >](::std::string::String),
            [< $field:camel Lt >](::std::string::String),
            [< $field:camel Lte >](::std::string::String),
            [< $field:camel Contains >](::std::string::String),
            [< $field:camel StartsWith >](::std::string::String),
            [< $field:camel EndsWith >](::std::string::String),
            [< $field:camel In >](::std::vec::Vec<::std::string::String>),
        }
    };
    ($field:ident, $other:ty) => {};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_try_bool {
    ($field:ident, bool) => {
        $crate::paste::paste! {
            [< $field:camel Eq >](bool),
            [< $field:camel Ne >](bool),
        }
    };
    ($field:ident, $other:ty) => {};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_try_opt_i64 {
    ($field:ident, Option<i64>) => {
        $crate::__prisma_where_try_opt_i64!($field, ::std::option::Option<i64>);
    };
    ($field:ident, ::std::option::Option<i64>) => {
        $crate::paste::paste! {
            [< $field:camel Eq >](::std::option::Option<i64>),
            [< $field:camel Ne >](::std::option::Option<i64>),
            [< $field:camel Gt >](i64),
            [< $field:camel Gte >](i64),
            [< $field:camel Lt >](i64),
            [< $field:camel Lte >](i64),
            [< $field:camel In >](::std::vec::Vec<i64>),
            [< $field:camel IsNull >],
            [< $field:camel IsNotNull >],
        }
    };
    ($field:ident, $other:ty) => {};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_try_opt_string {
    ($field:ident, Option<String>) => {
        $crate::__prisma_where_try_opt_string!($field, ::std::option::Option<String>);
    };
    ($field:ident, ::std::option::Option<String>) => {
        $crate::paste::paste! {
            [< $field:camel Eq >](::std::option::Option<::std::string::String>),
            [< $field:camel Ne >](::std::option::Option<::std::string::String>),
            [< $field:camel Gt >](::std::string::String),
            [< $field:camel Gte >](::std::string::String),
            [< $field:camel Lt >](::std::string::String),
            [< $field:camel Lte >](::std::string::String),
            [< $field:camel Contains >](::std::string::String),
            [< $field:camel StartsWith >](::std::string::String),
            [< $field:camel EndsWith >](::std::string::String),
            [< $field:camel In >](::std::vec::Vec<::std::string::String>),
            [< $field:camel IsNull >],
            [< $field:camel IsNotNull >],
        }
    };
    ($field:ident, $other:ty) => {};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_try_opt_bool {
    ($field:ident, ::std::option::Option<bool>) => {
        $crate::paste::paste! {
            [< $field:camel Eq >](::std::option::Option<bool>),
            [< $field:camel Ne >](::std::option::Option<bool>),
            [< $field:camel IsNull >],
            [< $field:camel IsNotNull >],
        }
    };
    ($field:ident, $other:ty) => {};
}

#[cfg(feature = "chrono")]
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_try_chrono_utc {
    ($field:ident, chrono::DateTime<chrono::Utc>) => {
        $crate::paste::paste! {
            [< $field:camel Eq >](chrono::DateTime<chrono::Utc>),
            [< $field:camel Ne >](chrono::DateTime<chrono::Utc>),
            [< $field:camel Gt >](chrono::DateTime<chrono::Utc>),
            [< $field:camel Gte >](chrono::DateTime<chrono::Utc>),
            [< $field:camel Lt >](chrono::DateTime<chrono::Utc>),
            [< $field:camel Lte >](chrono::DateTime<chrono::Utc>),
        }
    };
    ($field:ident, $other:ty) => {};
}

#[cfg(feature = "chrono")]
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_try_chrono_naive_datetime {
    ($field:ident, chrono::NaiveDateTime) => {
        $crate::paste::paste! {
            [< $field:camel Eq >](chrono::NaiveDateTime),
            [< $field:camel Ne >](chrono::NaiveDateTime),
            [< $field:camel Gt >](chrono::NaiveDateTime),
            [< $field:camel Gte >](chrono::NaiveDateTime),
            [< $field:camel Lt >](chrono::NaiveDateTime),
            [< $field:camel Lte >](chrono::NaiveDateTime),
        }
    };
    ($field:ident, $other:ty) => {};
}

#[cfg(not(feature = "chrono"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_try_chrono_utc {
    ($field:ident, $t:ty) => {};
}

#[cfg(not(feature = "chrono"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_try_chrono_naive_datetime {
    ($field:ident, $t:ty) => {};
}

#[cfg(feature = "uuid")]
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_try_uuid {
    ($field:ident, uuid::Uuid) => {
        $crate::paste::paste! {
            [< $field:camel Eq >](uuid::Uuid),
            [< $field:camel Ne >](uuid::Uuid),
            [< $field:camel Gt >](uuid::Uuid),
            [< $field:camel Gte >](uuid::Uuid),
            [< $field:camel Lt >](uuid::Uuid),
            [< $field:camel Lte >](uuid::Uuid),
            [< $field:camel In >](::std::vec::Vec<uuid::Uuid>),
        }
    };
    ($field:ident, $other:ty) => {};
}

#[cfg(not(feature = "uuid"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_try_uuid {
    ($field:ident, $t:ty) => {};
}

#[cfg(feature = "uuid")]
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_try_opt_uuid {
    ($field:ident, ::std::option::Option<uuid::Uuid>) => {
        $crate::paste::paste! {
            [< $field:camel Eq >](::std::option::Option<uuid::Uuid>),
            [< $field:camel Ne >](::std::option::Option<uuid::Uuid>),
            [< $field:camel In >](::std::vec::Vec<uuid::Uuid>),
            [< $field:camel IsNull >],
            [< $field:camel IsNotNull >],
        }
    };
    ($field:ident, $other:ty) => {};
}

#[cfg(not(feature = "uuid"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_try_opt_uuid {
    ($field:ident, $t:ty) => {};
}

/// Match arms for `__push_where` (same try pattern).
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_match_i64 {
    ($Self:ident, $field:ident, i64) => {
        $crate::paste::paste! {
            $Self::[< $field:camel Eq >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" = ");
                qb.push_bind(*v);
                Ok(())
            }
            $Self::[< $field:camel Ne >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" <> ");
                qb.push_bind(*v);
                Ok(())
            }
            $Self::[< $field:camel Gt >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" > ");
                qb.push_bind(*v);
                Ok(())
            }
            $Self::[< $field:camel Gte >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" >= ");
                qb.push_bind(*v);
                Ok(())
            }
            $Self::[< $field:camel Lt >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" < ");
                qb.push_bind(*v);
                Ok(())
            }
            $Self::[< $field:camel Lte >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" <= ");
                qb.push_bind(*v);
                Ok(())
            }
            $Self::[< $field:camel In >](v) => {
                if v.is_empty() {
                    qb.push("1=0");
                    return Ok(());
                }
                qb.push(concat!("\"", stringify!($field), "\" IN ("));
                let mut sep = qb.separated(", ");
                for x in v {
                    sep.push_bind(*x);
                }
                qb.push(")");
                Ok(())
            }
        }
    };
    ($Self:ident, $field:ident, $other:ty) => {};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_match_string {
    ($Self:ident, $field:ident, String) => {
        $crate::paste::paste! {
            $Self::[< $field:camel Eq >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" = ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Ne >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" <> ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Gt >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" > ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Gte >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" >= ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Lt >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" < ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Lte >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" <= ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Contains >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" LIKE ");
                let pat = format!("%{}%", v.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
                qb.push_bind(pat);
                qb.push(" ESCAPE '\\' ");
                Ok(())
            }
            $Self::[< $field:camel StartsWith >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" LIKE ");
                let pat = format!("{}%", v.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
                qb.push_bind(pat);
                qb.push(" ESCAPE '\\' ");
                Ok(())
            }
            $Self::[< $field:camel EndsWith >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" LIKE ");
                let pat = format!("%{}", v.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
                qb.push_bind(pat);
                qb.push(" ESCAPE '\\' ");
                Ok(())
            }
            $Self::[< $field:camel In >](v) => {
                if v.is_empty() {
                    qb.push("1=0");
                    return Ok(());
                }
                qb.push(concat!("\"", stringify!($field), "\" IN ("));
                let mut sep = qb.separated(", ");
                for x in v {
                    sep.push_bind(x.clone());
                }
                qb.push(")");
                Ok(())
            }
        }
    };
    ($Self:ident, $field:ident, $other:ty) => {};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_match_bool {
    ($Self:ident, $field:ident, bool) => {
        $crate::paste::paste! {
            $Self::[< $field:camel Eq >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" = ");
                qb.push_bind(*v);
                Ok(())
            }
            $Self::[< $field:camel Ne >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" <> ");
                qb.push_bind(*v);
                Ok(())
            }
        }
    };
    ($Self:ident, $field:ident, $other:ty) => {};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_match_opt_i64 {
    ($Self:ident, $field:ident, Option<i64>) => {
        $crate::__prisma_where_match_opt_i64!($Self, $field, ::std::option::Option<i64>);
    };
    ($Self:ident, $field:ident, ::std::option::Option<i64>) => {
        $crate::paste::paste! {
            $Self::[< $field:camel Eq >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" IS NOT DISTINCT FROM ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Ne >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" IS DISTINCT FROM ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Gt >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" > ");
                qb.push_bind(*v);
                Ok(())
            }
            $Self::[< $field:camel Gte >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" >= ");
                qb.push_bind(*v);
                Ok(())
            }
            $Self::[< $field:camel Lt >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" < ");
                qb.push_bind(*v);
                Ok(())
            }
            $Self::[< $field:camel Lte >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" <= ");
                qb.push_bind(*v);
                Ok(())
            }
            $Self::[< $field:camel In >](v) => {
                if v.is_empty() {
                    qb.push("1=0");
                    return Ok(());
                }
                qb.push(concat!("\"", stringify!($field), "\" IN ("));
                let mut sep = qb.separated(", ");
                for x in v {
                    sep.push_bind(*x);
                }
                qb.push(")");
                Ok(())
            }
            $Self::[< $field:camel IsNull >] => {
                qb.push(concat!("\"", stringify!($field), "\" IS NULL"));
                Ok(())
            }
            $Self::[< $field:camel IsNotNull >] => {
                qb.push(concat!("\"", stringify!($field), "\" IS NOT NULL"));
                Ok(())
            }
        }
    };
    ($Self:ident, $field:ident, $other:ty) => {};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_match_opt_string {
    ($Self:ident, $field:ident, Option<String>) => {
        $crate::__prisma_where_match_opt_string!($Self, $field, ::std::option::Option<String>);
    };
    ($Self:ident, $field:ident, ::std::option::Option<String>) => {
        $crate::paste::paste! {
            $Self::[< $field:camel Eq >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" IS NOT DISTINCT FROM ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Ne >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" IS DISTINCT FROM ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Gt >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" > ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Gte >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" >= ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Lt >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" < ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Lte >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" <= ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Contains >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" LIKE ");
                let pat = format!("%{}%", v.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
                qb.push_bind(pat);
                qb.push(" ESCAPE '\\' ");
                Ok(())
            }
            $Self::[< $field:camel StartsWith >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" LIKE ");
                let pat = format!("{}%", v.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
                qb.push_bind(pat);
                qb.push(" ESCAPE '\\' ");
                Ok(())
            }
            $Self::[< $field:camel EndsWith >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" LIKE ");
                let pat = format!("%{}", v.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
                qb.push_bind(pat);
                qb.push(" ESCAPE '\\' ");
                Ok(())
            }
            $Self::[< $field:camel In >](v) => {
                if v.is_empty() {
                    qb.push("1=0");
                    return Ok(());
                }
                qb.push(concat!("\"", stringify!($field), "\" IN ("));
                let mut sep = qb.separated(", ");
                for x in v {
                    sep.push_bind(x.clone());
                }
                qb.push(")");
                Ok(())
            }
            $Self::[< $field:camel IsNull >] => {
                qb.push(concat!("\"", stringify!($field), "\" IS NULL"));
                Ok(())
            }
            $Self::[< $field:camel IsNotNull >] => {
                qb.push(concat!("\"", stringify!($field), "\" IS NOT NULL"));
                Ok(())
            }
        }
    };
    ($Self:ident, $field:ident, $other:ty) => {};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_match_opt_bool {
    ($Self:ident, $field:ident, ::std::option::Option<bool>) => {
        $crate::paste::paste! {
            $Self::[< $field:camel Eq >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" IS NOT DISTINCT FROM ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Ne >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" IS DISTINCT FROM ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel IsNull >] => {
                qb.push(concat!("\"", stringify!($field), "\" IS NULL"));
                Ok(())
            }
            $Self::[< $field:camel IsNotNull >] => {
                qb.push(concat!("\"", stringify!($field), "\" IS NOT NULL"));
                Ok(())
            }
        }
    };
    ($Self:ident, $field:ident, $other:ty) => {};
}

#[cfg(feature = "chrono")]
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_match_chrono_utc {
    ($Self:ident, $field:ident, chrono::DateTime<chrono::Utc>) => {
        $crate::paste::paste! {
            $Self::[< $field:camel Eq >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" = ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Ne >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" <> ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Gt >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" > ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Gte >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" >= ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Lt >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" < ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Lte >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" <= ");
                qb.push_bind(v.clone());
                Ok(())
            }
        }
    };
    ($Self:ident, $field:ident, $other:ty) => {};
}

#[cfg(feature = "chrono")]
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_match_chrono_naive_datetime {
    ($Self:ident, $field:ident, chrono::NaiveDateTime) => {
        $crate::paste::paste! {
            $Self::[< $field:camel Eq >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" = ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Ne >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" <> ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Gt >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" > ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Gte >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" >= ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Lt >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" < ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Lte >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" <= ");
                qb.push_bind(v.clone());
                Ok(())
            }
        }
    };
    ($Self:ident, $field:ident, $other:ty) => {};
}

#[cfg(not(feature = "chrono"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_match_chrono_utc {
    ($Self:ident, $field:ident, $t:ty) => {};
}

#[cfg(not(feature = "chrono"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_match_chrono_naive_datetime {
    ($Self:ident, $field:ident, $t:ty) => {};
}

#[cfg(feature = "uuid")]
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_match_uuid {
    ($Self:ident, $field:ident, uuid::Uuid) => {
        $crate::paste::paste! {
            $Self::[< $field:camel Eq >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" = ");
                qb.push_bind(*v);
                Ok(())
            }
            $Self::[< $field:camel Ne >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" <> ");
                qb.push_bind(*v);
                Ok(())
            }
            $Self::[< $field:camel Gt >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" > ");
                qb.push_bind(*v);
                Ok(())
            }
            $Self::[< $field:camel Gte >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" >= ");
                qb.push_bind(*v);
                Ok(())
            }
            $Self::[< $field:camel Lt >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" < ");
                qb.push_bind(*v);
                Ok(())
            }
            $Self::[< $field:camel Lte >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" <= ");
                qb.push_bind(*v);
                Ok(())
            }
            $Self::[< $field:camel In >](v) => {
                if v.is_empty() {
                    qb.push("1=0");
                    return Ok(());
                }
                qb.push(concat!("\"", stringify!($field), "\" IN ("));
                let mut sep = qb.separated(", ");
                for x in v {
                    sep.push_bind(*x);
                }
                qb.push(")");
                Ok(())
            }
        }
    };
    ($Self:ident, $field:ident, $other:ty) => {};
}

#[cfg(not(feature = "uuid"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_match_uuid {
    ($Self:ident, $field:ident, $t:ty) => {};
}

#[cfg(feature = "uuid")]
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_match_opt_uuid {
    ($Self:ident, $field:ident, ::std::option::Option<uuid::Uuid>) => {
        $crate::paste::paste! {
            $Self::[< $field:camel Eq >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" IS NOT DISTINCT FROM ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel Ne >](v) => {
                qb.push(concat!("\"", stringify!($field), "\""));
                qb.push(" IS DISTINCT FROM ");
                qb.push_bind(v.clone());
                Ok(())
            }
            $Self::[< $field:camel In >](v) => {
                if v.is_empty() {
                    qb.push("1=0");
                    return Ok(());
                }
                qb.push(concat!("\"", stringify!($field), "\" IN ("));
                let mut sep = qb.separated(", ");
                for x in v {
                    sep.push_bind(*x);
                }
                qb.push(")");
                Ok(())
            }
            $Self::[< $field:camel IsNull >] => {
                qb.push(concat!("\"", stringify!($field), "\" IS NULL"));
                Ok(())
            }
            $Self::[< $field:camel IsNotNull >] => {
                qb.push(concat!("\"", stringify!($field), "\" IS NOT NULL"));
                Ok(())
            }
        }
    };
    ($Self:ident, $field:ident, $other:ty) => {};
}

#[cfg(not(feature = "uuid"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_match_opt_uuid {
    ($Self:ident, $field:ident, $t:ty) => {};
}

/// Field helper fns: `gt`, `contains`, … (only matching type expands).
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_field_helpers_try_i64 {
    ($field:ident, $ftype:ty, $Where:ident, $Order:ident, $Update:ident) => {
        $crate::paste::paste! {
            pub fn gt(v: i64) -> $Where { $Where::[< $field:camel Gt >](v) }
            pub fn gte(v: i64) -> $Where { $Where::[< $field:camel Gte >](v) }
            pub fn lt(v: i64) -> $Where { $Where::[< $field:camel Lt >](v) }
            pub fn lte(v: i64) -> $Where { $Where::[< $field:camel Lte >](v) }
            pub fn in_list(v: ::std::vec::Vec<i64>) -> $Where { $Where::[< $field:camel In >](v) }
        }
    };
    ($field:ident, $other:ty, $Where:ident, $Order:ident, $Update:ident) => {};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_field_helpers_try_string {
    ($field:ident, String, $Where:ident, $Order:ident, $Update:ident) => {
        $crate::paste::paste! {
            pub fn gt(v: ::std::string::String) -> $Where { $Where::[< $field:camel Gt >](v) }
            pub fn gte(v: ::std::string::String) -> $Where { $Where::[< $field:camel Gte >](v) }
            pub fn lt(v: ::std::string::String) -> $Where { $Where::[< $field:camel Lt >](v) }
            pub fn lte(v: ::std::string::String) -> $Where { $Where::[< $field:camel Lte >](v) }
            pub fn contains(v: ::std::string::String) -> $Where { $Where::[< $field:camel Contains >](v) }
            pub fn starts_with(v: ::std::string::String) -> $Where { $Where::[< $field:camel StartsWith >](v) }
            pub fn ends_with(v: ::std::string::String) -> $Where { $Where::[< $field:camel EndsWith >](v) }
            pub fn in_list(v: ::std::vec::Vec<::std::string::String>) -> $Where { $Where::[< $field:camel In >](v) }
        }
    };
    ($field:ident, $other:ty, $Where:ident, $Order:ident, $Update:ident) => {};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_field_helpers_try_opt {
    ($field:ident, Option<i64>, $Where:ident, $Order:ident, $Update:ident) => {
        $crate::__prisma_field_helpers_try_opt!(
            $field,
            ::std::option::Option<i64>,
            $Where,
            $Order,
            $Update
        );
    };
    ($field:ident, Option<String>, $Where:ident, $Order:ident, $Update:ident) => {
        $crate::__prisma_field_helpers_try_opt!(
            $field,
            ::std::option::Option<String>,
            $Where,
            $Order,
            $Update
        );
    };
    ($field:ident, Option<bool>, $Where:ident, $Order:ident, $Update:ident) => {
        $crate::__prisma_field_helpers_try_opt!(
            $field,
            ::std::option::Option<bool>,
            $Where,
            $Order,
            $Update
        );
    };
    ($field:ident, ::std::option::Option<i64>, $Where:ident, $Order:ident, $Update:ident) => {
        $crate::paste::paste! {
            pub fn is_null() -> $Where { $Where::[< $field:camel IsNull >] }
            pub fn is_not_null() -> $Where { $Where::[< $field:camel IsNotNull >] }
        }
    };
    ($field:ident, ::std::option::Option<String>, $Where:ident, $Order:ident, $Update:ident) => {
        $crate::paste::paste! {
            pub fn is_null() -> $Where { $Where::[< $field:camel IsNull >] }
            pub fn is_not_null() -> $Where { $Where::[< $field:camel IsNotNull >] }
        }
    };
    ($field:ident, ::std::option::Option<bool>, $Where:ident, $Order:ident, $Update:ident) => {
        $crate::paste::paste! {
            pub fn is_null() -> $Where { $Where::[< $field:camel IsNull >] }
            pub fn is_not_null() -> $Where { $Where::[< $field:camel IsNotNull >] }
        }
    };
    ($field:ident, $other:ty, $Where:ident, $Order:ident, $Update:ident) => {};
}

#[cfg(feature = "uuid")]
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_field_helpers_try_opt_uuid {
    ($field:ident, ::std::option::Option<uuid::Uuid>, $Where:ident, $Order:ident, $Update:ident) => {
        $crate::paste::paste! {
            pub fn is_null() -> $Where { $Where::[< $field:camel IsNull >] }
            pub fn is_not_null() -> $Where { $Where::[< $field:camel IsNotNull >] }
        }
    };
    ($field:ident, $other:ty, $Where:ident, $Order:ident, $Update:ident) => {};
}

#[cfg(not(feature = "uuid"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_field_helpers_try_opt_uuid {
    ($field:ident, $other:ty, $Where:ident, $Order:ident, $Update:ident) => {};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_field_helpers_try_opt_i64_ops {
    ($field:ident, Option<i64>, $Where:ident, $Order:ident, $Update:ident) => {
        $crate::__prisma_field_helpers_try_opt_i64_ops!(
            $field,
            ::std::option::Option<i64>,
            $Where,
            $Order,
            $Update
        );
    };
    ($field:ident, ::std::option::Option<i64>, $Where:ident, $Order:ident, $Update:ident) => {
        $crate::paste::paste! {
            pub fn gt(v: i64) -> $Where {
                $Where::[< $field:camel Gt >](v)
            }
            pub fn gte(v: i64) -> $Where {
                $Where::[< $field:camel Gte >](v)
            }
            pub fn lt(v: i64) -> $Where {
                $Where::[< $field:camel Lt >](v)
            }
            pub fn lte(v: i64) -> $Where {
                $Where::[< $field:camel Lte >](v)
            }
            pub fn in_list(v: ::std::vec::Vec<i64>) -> $Where {
                $Where::[< $field:camel In >](v)
            }
        }
    };
    ($field:ident, $other:ty, $Where:ident, $Order:ident, $Update:ident) => {};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_field_helpers_try_opt_string_ops {
    ($field:ident, Option<String>, $Where:ident, $Order:ident, $Update:ident) => {
        $crate::__prisma_field_helpers_try_opt_string_ops!(
            $field,
            ::std::option::Option<String>,
            $Where,
            $Order,
            $Update
        );
    };
    ($field:ident, ::std::option::Option<String>, $Where:ident, $Order:ident, $Update:ident) => {
        $crate::paste::paste! {
            pub fn gt(v: ::std::string::String) -> $Where {
                $Where::[< $field:camel Gt >](v)
            }
            pub fn gte(v: ::std::string::String) -> $Where {
                $Where::[< $field:camel Gte >](v)
            }
            pub fn lt(v: ::std::string::String) -> $Where {
                $Where::[< $field:camel Lt >](v)
            }
            pub fn lte(v: ::std::string::String) -> $Where {
                $Where::[< $field:camel Lte >](v)
            }
            pub fn contains(v: ::std::string::String) -> $Where {
                $Where::[< $field:camel Contains >](v)
            }
            pub fn starts_with(v: ::std::string::String) -> $Where {
                $Where::[< $field:camel StartsWith >](v)
            }
            pub fn ends_with(v: ::std::string::String) -> $Where {
                $Where::[< $field:camel EndsWith >](v)
            }
            pub fn in_list(v: ::std::vec::Vec<::std::string::String>) -> $Where {
                $Where::[< $field:camel In >](v)
            }
        }
    };
    ($field:ident, $other:ty, $Where:ident, $Order:ident, $Update:ident) => {};
}

/// Emits `{Model}Where` + `__push_where` in a **single** `paste!` (avoids nesting `paste!` inside `prisma_model!`'s outer `paste!`).
///
/// Variant set is intentionally **minimal** (Eq/Ne only): `paste!` cannot reliably splice nested macro
/// invocations into enum variant lists; full operator surfaces live in `macros_where.rs` for a future
/// proc-macro or non-`paste` codegen path.
#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_where_types {
    ($Model:ident; $( ($field:ident, $ftype:ty) ),* $(,)?) => {
        $crate::paste::paste! {
            #[derive(Debug, Clone)]
            pub enum [< $Model Where >] {
                And(Vec<[<$Model Where>]>),
                Or(Vec<[<$Model Where>]>),
                Not(Box<[<$Model Where>]>),
                $(
                    [< $field:camel Eq >]($ftype),
                    [< $field:camel Ne >]($ftype),
                )*
            }

            impl [< $Model Where >] {
                pub fn and(parts: Vec<Self>) -> Self {
                    Self::And(parts)
                }
                pub fn or(parts: Vec<Self>) -> Self {
                    Self::Or(parts)
                }
                pub fn not(inner: Self) -> Self {
                    Self::Not(Box::new(inner))
                }

                fn __push_where(
                    &self,
                    qb: &mut $crate::sqlx::QueryBuilder<'_, $crate::SqlxDb>,
                ) -> std::result::Result<(), $crate::PrismaError> {
                    match self {
                        Self::And(parts) => {
                            if parts.is_empty() {
                                qb.push("1=1");
                                return Ok(());
                            }
                            qb.push("(");
                            for (i, p) in parts.iter().enumerate() {
                                if i > 0 {
                                    qb.push(" AND ");
                                }
                                qb.push("(");
                                p.__push_where(qb)?;
                                qb.push(")");
                            }
                            qb.push(")");
                            Ok(())
                        }
                        Self::Or(parts) => {
                            if parts.is_empty() {
                                qb.push("1=0");
                                return Ok(());
                            }
                            qb.push("(");
                            for (i, p) in parts.iter().enumerate() {
                                if i > 0 {
                                    qb.push(" OR ");
                                }
                                qb.push("(");
                                p.__push_where(qb)?;
                                qb.push(")");
                            }
                            qb.push(")");
                            Ok(())
                        }
                        Self::Not(inner) => {
                            qb.push("NOT (");
                            inner.__push_where(qb)?;
                            qb.push(")");
                            Ok(())
                        }
                        $(
                            Self::[< $field:camel Eq >](v) => {
                                qb.push(concat!("\"", stringify!($field), "\""));
                                qb.push(" = ");
                                qb.push_bind(v.clone());
                                Ok(())
                            }
                            Self::[< $field:camel Ne >](v) => {
                                qb.push(concat!("\"", stringify!($field), "\""));
                                qb.push(" <> ");
                                qb.push_bind(v.clone());
                                Ok(())
                            }
                        )*
                    }
                }
            }

            impl Default for [< $Model Where >] {
                fn default() -> Self {
                    Self::And(Vec::new())
                }
            }
        }
    };
}
