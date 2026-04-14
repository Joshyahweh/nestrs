//! Prisma-style enums mapped to SQL `TEXT` via SQLx `Any` (Postgres / SQLite).

#[cfg(not(feature = "sqlx"))]
#[macro_export]
macro_rules! prisma_enum {
    ($($t:tt)*) => {
        ::core::compile_error!("prisma_enum! requires the `sqlx` feature on nestrs-prisma");
    };
}

#[cfg(feature = "sqlx")]
#[macro_export]
macro_rules! prisma_enum {
    ($Name:ident { $( $v:ident ),* $(,)? }) => {
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
            serde::Serialize,
            serde::Deserialize,
        )]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum $Name {
            $( $v ),*
        }

        impl $crate::sqlx::Type<$crate::sqlx::Any> for $Name {
            fn type_info() -> $crate::sqlx::any::AnyTypeInfo {
                <str as $crate::sqlx::Type<$crate::sqlx::Any>>::type_info()
            }
        }

        impl $Name {
            pub fn as_str(&self) -> &'static str {
                match self {
                    $( $Name::$v => stringify!($v), )*
                }
            }
        }

        impl ::std::str::FromStr for $Name {
            type Err = ::std::string::String;

            fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                match s {
                    $( stringify!($v) => ::std::result::Result::Ok($Name::$v), )*
                    _ => ::std::result::Result::Err(format!("unknown enum variant: {s}")),
                }
            }
        }

        impl<'q> $crate::sqlx::Encode<'q, $crate::sqlx::Any> for $Name {
            fn encode_by_ref(
                &self,
                buf: &mut ::std::vec::Vec<$crate::sqlx::any::AnyArgumentBuffer<'q>>,
            ) -> $crate::sqlx::encode::IsNull {
                <&str as $crate::sqlx::Encode<'q, $crate::sqlx::Any>>::encode(self.as_str(), buf)
            }

            fn size_hint(&self) -> usize {
                <&str as $crate::sqlx::Encode<'q, $crate::sqlx::Any>>::size_hint(&self.as_str())
            }
        }

        impl<'r> $crate::sqlx::Decode<'r, $crate::sqlx::Any> for $Name {
            fn decode(value: $crate::sqlx::any::AnyValueRef<'r>) -> ::std::result::Result<Self, $crate::sqlx::error::BoxDynError> {
                let s = <&str as $crate::sqlx::Decode<'r, $crate::sqlx::Any>>::decode(value)?;
                s.parse::<$Name>().map_err(|e: ::std::string::String| e.into())
            }
        }
    };
}
