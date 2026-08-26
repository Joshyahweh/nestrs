//! Macros for ergonomic relation schema declaration.

/// Builds [`crate::relations::RelationDefinition`] values with Prisma-like relation intent.
///
/// Supported forms:
/// - `one_to_one`
/// - `one_to_many`
/// - `many_to_many_implicit`
/// - `many_to_many_explicit`
#[macro_export]
macro_rules! prisma_relation {
    (
        one_to_one,
        name: $name:expr,
        left: {
            model: $l_model:expr,
            field: $l_field:expr,
            optional: $l_optional:expr
        },
        right: {
            model: $r_model:expr,
            field: $r_field:expr,
            optional: $r_optional:expr,
            scalar: [ $( $r_scalar:expr ),+ $(,)? ],
            references: [ $( $r_ref:expr ),+ $(,)? ],
            unique: $r_unique:expr,
            indexed: $r_indexed:expr
        },
        on_delete: $on_delete:ident,
        on_update: $on_update:ident
    ) => {{
        $crate::relations::RelationDefinition::new(
            $crate::relations::RelationKind::OneToOne,
            $crate::relations::RelationEndpoint::new($l_model, $l_field).optional($l_optional),
            $crate::relations::RelationEndpoint::new($r_model, $r_field)
                .optional($r_optional)
                .scalar(vec![ $( $r_scalar ),+ ], vec![ $( $r_ref ),+ ])
                .unique($r_unique)
                .indexed($r_indexed),
        )
        .name($name)
        .on_delete($crate::relations::ReferentialAction::$on_delete)
        .on_update($crate::relations::ReferentialAction::$on_update)
    }};

    (
        one_to_many,
        name: $name:expr,
        parent: {
            model: $p_model:expr,
            field: $p_field:expr
        },
        child: {
            model: $c_model:expr,
            field: $c_field:expr,
            optional: $c_optional:expr,
            scalar: [ $( $c_scalar:expr ),+ $(,)? ],
            references: [ $( $c_ref:expr ),+ $(,)? ],
            indexed: $c_indexed:expr
        },
        on_delete: $on_delete:ident,
        on_update: $on_update:ident
    ) => {{
        $crate::relations::RelationDefinition::new(
            $crate::relations::RelationKind::OneToMany,
            $crate::relations::RelationEndpoint::new($p_model, $p_field).list(true),
            $crate::relations::RelationEndpoint::new($c_model, $c_field)
                .optional($c_optional)
                .scalar(vec![ $( $c_scalar ),+ ], vec![ $( $c_ref ),+ ])
                .indexed($c_indexed),
        )
        .name($name)
        .on_delete($crate::relations::ReferentialAction::$on_delete)
        .on_update($crate::relations::ReferentialAction::$on_update)
    }};

    (
        many_to_many_implicit,
        name: $name:expr,
        left: { model: $l_model:expr, field: $l_field:expr },
        right: { model: $r_model:expr, field: $r_field:expr }
    ) => {{
        $crate::relations::RelationDefinition::new(
            $crate::relations::RelationKind::ManyToManyImplicit,
            $crate::relations::RelationEndpoint::new($l_model, $l_field).list(true),
            $crate::relations::RelationEndpoint::new($r_model, $r_field).list(true),
        )
        .name($name)
    }};

    (
        many_to_many_explicit,
        name: $name:expr,
        left: { model: $l_model:expr, field: $l_field:expr },
        right: { model: $r_model:expr, field: $r_field:expr },
        join: {
            model: $j_model:expr,
            has_primary_key: $j_has_pk:expr,
            left_back_relation: $j_left_back:expr,
            right_back_relation: $j_right_back:expr
        }
    ) => {{
        $crate::relations::RelationDefinition::new(
            $crate::relations::RelationKind::ManyToManyExplicit,
            $crate::relations::RelationEndpoint::new($l_model, $l_field).list(true),
            $crate::relations::RelationEndpoint::new($r_model, $r_field).list(true),
        )
        .name($name)
        .join_model($crate::relations::JoinModel {
            model: $j_model.to_string(),
            has_primary_key: $j_has_pk,
            left_back_relation_present: $j_left_back,
            right_back_relation_present: $j_right_back,
            extra_fields: vec![],
        })
    }};
}

/// Builds a [`crate::relations::RelationSchema`] with models and relations.
#[macro_export]
macro_rules! prisma_relation_schema {
    (
        relation_mode: $mode:ident,
        dialect: $dialect:ident,
        models: [ $( ($m_name:expr, single_id: $m_single:expr) ),* $(,)? ],
        relations: [ $( $rel:expr ),* $(,)? ]
    ) => {{
        let schema = $crate::relations::RelationSchema::new(
            $crate::relations::RelationMode::$mode,
            $crate::index_ddl::SqlDialect::$dialect,
        )
        $(
            .model($crate::relations::ModelMetadata {
                name: $m_name.to_string(),
                single_id: $m_single,
            })
        )*
        $(
            .relation($rel)
        )*;
        schema
    }};
}

/// Generates model-specific relation query methods on `ModelRepository<Model>`.
///
/// This lets users keep Prisma-like ergonomics (`prisma.user().include_posts(...)`) while
/// still controlling the exact table/column mapping.
///
/// Example:
/// ```ignore
/// nestrs_prisma::prisma_model_relations!(User {
///     (one_to_many posts: Post { child_table: "posts", child_fk: "author_id" })
///     (one_to_one profile: Profile { table: "profiles", fk: "user_id" })
///     (foreign_key profile_owner { table: "profiles", record_pk: "id", fk: "user_id", nullable: true })
/// });
/// ```
#[macro_export]
macro_rules! prisma_model_relations {
    ($Model:ident { $( ( $($entry:tt)+ ) )* }) => {
        $crate::paste::paste! {
            #[::async_trait::async_trait]
            pub trait [< Prisma $Model RelationRepository >]: Send + Sync {
                $( $crate::prisma_model_relations!(@trait_method $($entry)+); )*
            }

            #[::async_trait::async_trait]
            impl [< Prisma $Model RelationRepository >] for $crate::client::ModelRepository<$Model> {
                $( $crate::prisma_model_relations!(@impl_method $($entry)+); )*
            }
        }
    };

    (@trait_method one_to_many $many_name:ident : $ManyTarget:ty {
        child_table: $many_table:literal,
        child_fk: $many_fk:literal
    }) => {
        $crate::paste::paste! {
            async fn [< include_ $many_name >](
                &self,
                parent_id: $crate::relation_queries::RelationIdValue,
                opts: $crate::relation_queries::IncludeOptions,
            ) -> std::result::Result<Vec<$ManyTarget>, $crate::PrismaError>;
        }
    };
    (@trait_method one_to_one $one_name:ident : $OneTarget:ty {
        table: $one_table:literal,
        fk: $one_fk:literal
    }) => {
        $crate::paste::paste! {
            async fn [< include_ $one_name >](
                &self,
                owner_id: $crate::relation_queries::RelationIdValue,
            ) -> std::result::Result<std::option::Option<$OneTarget>, $crate::PrismaError>;
        }
    };
    (@trait_method many_to_many $m2m_name:ident : $M2mTarget:ty {
        related_table: $m2m_related_table:literal,
        related_pk: $m2m_related_pk:literal,
        join_table: $m2m_join_table:literal,
        join_left: $m2m_join_left:literal,
        join_right: $m2m_join_right:literal
    }) => {
        $crate::paste::paste! {
            async fn [< include_ $m2m_name >](
                &self,
                owner_id: $crate::relation_queries::RelationIdValue,
                opts: $crate::relation_queries::IncludeOptions,
            ) -> std::result::Result<Vec<$M2mTarget>, $crate::PrismaError>;
        }
    };
    (@trait_method foreign_key $fk_name:ident {
        table: $fk_table:literal,
        record_pk: $fk_record_pk:literal,
        fk: $fk_column:literal,
        nullable: $fk_nullable:literal
    }) => {
        $crate::paste::paste! {
            async fn [< connect_ $fk_name >](
                &self,
                record_id: $crate::relation_queries::RelationIdValue,
                target_id: $crate::relation_queries::RelationIdValue,
            ) -> std::result::Result<u64, $crate::PrismaError>;
            async fn [< disconnect_ $fk_name >](
                &self,
                record_id: $crate::relation_queries::RelationIdValue,
            ) -> std::result::Result<u64, $crate::PrismaError>;
        }
    };
    (@trait_method join_mutation $join_name:ident {
        join_table: $join_table:literal,
        left: $join_left:literal,
        right: $join_right:literal
    }) => {
        $crate::paste::paste! {
            async fn [< connect_ $join_name >](
                &self,
                left_id: $crate::relation_queries::RelationIdValue,
                right_id: $crate::relation_queries::RelationIdValue,
            ) -> std::result::Result<u64, $crate::PrismaError>;
            async fn [< disconnect_ $join_name >](
                &self,
                left_id: $crate::relation_queries::RelationIdValue,
                right_id: $crate::relation_queries::RelationIdValue,
            ) -> std::result::Result<u64, $crate::PrismaError>;
        }
    };

    (@impl_method one_to_many $many_name:ident : $ManyTarget:ty {
        child_table: $many_table:literal,
        child_fk: $many_fk:literal
    }) => {
        $crate::paste::paste! {
            async fn [< include_ $many_name >](
                &self,
                parent_id: $crate::relation_queries::RelationIdValue,
                opts: $crate::relation_queries::IncludeOptions,
            ) -> std::result::Result<Vec<$ManyTarget>, $crate::PrismaError> {
                self.prisma()
                    .include_one_to_many_as::<$ManyTarget>(
                        &$crate::relation_queries::OneToManyIncludeSpec::new(
                            $many_table,
                            $many_fk,
                        ),
                        parent_id,
                        opts,
                    )
                    .await
                    .map_err(|e| $crate::PrismaError::other(e.to_string()))
            }
        }
    };
    (@impl_method one_to_one $one_name:ident : $OneTarget:ty {
        table: $one_table:literal,
        fk: $one_fk:literal
    }) => {
        $crate::paste::paste! {
            async fn [< include_ $one_name >](
                &self,
                owner_id: $crate::relation_queries::RelationIdValue,
            ) -> std::result::Result<std::option::Option<$OneTarget>, $crate::PrismaError> {
                self.prisma()
                    .include_one_to_one_as::<$OneTarget>(
                        &$crate::relation_queries::OneToOneIncludeSpec::new(
                            $one_table,
                            $one_fk,
                        ),
                        owner_id,
                    )
                    .await
                    .map_err(|e| $crate::PrismaError::other(e.to_string()))
            }
        }
    };
    (@impl_method many_to_many $m2m_name:ident : $M2mTarget:ty {
        related_table: $m2m_related_table:literal,
        related_pk: $m2m_related_pk:literal,
        join_table: $m2m_join_table:literal,
        join_left: $m2m_join_left:literal,
        join_right: $m2m_join_right:literal
    }) => {
        $crate::paste::paste! {
            async fn [< include_ $m2m_name >](
                &self,
                owner_id: $crate::relation_queries::RelationIdValue,
                opts: $crate::relation_queries::IncludeOptions,
            ) -> std::result::Result<Vec<$M2mTarget>, $crate::PrismaError> {
                self.prisma()
                    .include_many_to_many_as::<$M2mTarget>(
                        &$crate::relation_queries::ManyToManyIncludeSpec::new(
                            $m2m_related_table,
                            $m2m_related_pk,
                            $m2m_join_table,
                            $m2m_join_left,
                            $m2m_join_right,
                        ),
                        owner_id,
                        opts,
                    )
                    .await
                    .map_err(|e| $crate::PrismaError::other(e.to_string()))
            }
        }
    };
    (@impl_method foreign_key $fk_name:ident {
        table: $fk_table:literal,
        record_pk: $fk_record_pk:literal,
        fk: $fk_column:literal,
        nullable: $fk_nullable:literal
    }) => {
        $crate::paste::paste! {
            async fn [< connect_ $fk_name >](
                &self,
                record_id: $crate::relation_queries::RelationIdValue,
                target_id: $crate::relation_queries::RelationIdValue,
            ) -> std::result::Result<u64, $crate::PrismaError> {
                self.prisma()
                    .connect_fk(
                        &$crate::relation_queries::ForeignKeyMutationSpec::new(
                            $fk_table,
                            $fk_record_pk,
                            $fk_column,
                            $fk_nullable,
                        ),
                        record_id,
                        target_id,
                    )
                    .await
                    .map_err(|e| $crate::PrismaError::other(e.to_string()))
            }
            async fn [< disconnect_ $fk_name >](
                &self,
                record_id: $crate::relation_queries::RelationIdValue,
            ) -> std::result::Result<u64, $crate::PrismaError> {
                self.prisma()
                    .disconnect_fk(
                        &$crate::relation_queries::ForeignKeyMutationSpec::new(
                            $fk_table,
                            $fk_record_pk,
                            $fk_column,
                            $fk_nullable,
                        ),
                        record_id,
                    )
                    .await
                    .map_err(|e| $crate::PrismaError::other(e.to_string()))
            }
        }
    };
    (@impl_method join_mutation $join_name:ident {
        join_table: $join_table:literal,
        left: $join_left:literal,
        right: $join_right:literal
    }) => {
        $crate::paste::paste! {
            async fn [< connect_ $join_name >](
                &self,
                left_id: $crate::relation_queries::RelationIdValue,
                right_id: $crate::relation_queries::RelationIdValue,
            ) -> std::result::Result<u64, $crate::PrismaError> {
                self.prisma()
                    .connect_many_to_many(
                        &$crate::relation_queries::JoinMutationSpec::new(
                            $join_table,
                            $join_left,
                            $join_right,
                        ),
                        left_id,
                        right_id,
                    )
                    .await
                    .map_err(|e| $crate::PrismaError::other(e.to_string()))
            }
            async fn [< disconnect_ $join_name >](
                &self,
                left_id: $crate::relation_queries::RelationIdValue,
                right_id: $crate::relation_queries::RelationIdValue,
            ) -> std::result::Result<u64, $crate::PrismaError> {
                self.prisma()
                    .disconnect_many_to_many(
                        &$crate::relation_queries::JoinMutationSpec::new(
                            $join_table,
                            $join_left,
                            $join_right,
                        ),
                        left_id,
                        right_id,
                    )
                    .await
                    .map_err(|e| $crate::PrismaError::other(e.to_string()))
            }
        }
    };
}
