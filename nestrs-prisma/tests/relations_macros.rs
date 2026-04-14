use nestrs_prisma::relations::{build_relation_deployment_plan, validate_relations};

#[test]
fn relation_macros_build_valid_schema_and_plan() {
    let user_posts = nestrs_prisma::prisma_relation!(
        one_to_many,
        name: "UserPosts",
        parent: { model: "User", field: "posts" },
        child: {
            model: "Post",
            field: "author",
            optional: false,
            scalar: ["authorId"],
            references: ["id"],
            indexed: true
        },
        on_delete: Cascade,
        on_update: Cascade
    );

    let user_profile = nestrs_prisma::prisma_relation!(
        one_to_one,
        name: "UserProfile",
        left: { model: "User", field: "profile", optional: true },
        right: {
            model: "Profile",
            field: "user",
            optional: false,
            scalar: ["userId"],
            references: ["id"],
            unique: true,
            indexed: true
        },
        on_delete: Cascade,
        on_update: Cascade
    );

    let post_categories = nestrs_prisma::prisma_relation!(
        many_to_many_implicit,
        name: "PostCategories",
        left: { model: "Post", field: "categories" },
        right: { model: "Category", field: "posts" }
    );

    let schema = nestrs_prisma::prisma_relation_schema!(
        relation_mode: ForeignKeys,
        dialect: PostgreSql,
        models: [
            ("User", single_id: true),
            ("Post", single_id: true),
            ("Profile", single_id: true),
            ("Category", single_id: true)
        ],
        relations: [user_posts, user_profile, post_categories]
    );

    let report = validate_relations(&schema).expect("schema should validate");
    assert!(report.index_recommendations.is_empty());

    let plan = build_relation_deployment_plan(&schema, 63).expect("deployment plan");
    // one_to_many + one_to_one each generate one FK DDL statement
    assert_eq!(plan.foreign_key_sql.len(), 2);
}

#[test]
fn explicit_many_to_many_macro_needs_valid_join_model() {
    let rel = nestrs_prisma::prisma_relation!(
        many_to_many_explicit,
        name: "PostCategoryJoin",
        left: { model: "Post", field: "postCategories" },
        right: { model: "Category", field: "postCategories" },
        join: {
            model: "PostCategories",
            has_primary_key: false,
            left_back_relation: true,
            right_back_relation: true
        }
    );

    let schema = nestrs_prisma::prisma_relation_schema!(
        relation_mode: ForeignKeys,
        dialect: PostgreSql,
        models: [
            ("Post", single_id: true),
            ("Category", single_id: true),
            ("PostCategories", single_id: true)
        ],
        relations: [rel]
    );

    let err = validate_relations(&schema).expect_err("join model PK is required");
    assert!(err.to_string().contains("primary key"));
}
