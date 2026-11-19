use sqlx::PgPool;

use crate::{
    accounts::contract::{DirectoryLookup, PlayerDirectory, PlayerName},
    platform::observability::database,
};

#[derive(Clone)]
pub(in crate::accounts) struct PostgresPlayerDirectory {
    pool: PgPool,
}

impl PostgresPlayerDirectory {
    pub(in crate::accounts) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    #[tracing::instrument(name = "accounts.directory.find", skip_all, fields(ddd.bounded_context.name = "accounts", input.ids.count = ids.len()), err)]
    async fn find_names(&self, ids: &[i64]) -> anyhow::Result<Vec<PlayerName>> {
        let statement = "SELECT id, display_name FROM accounts.accounts WHERE id = ANY($1)";
        let rows: Vec<(i64, String)> = database(
            "accounts",
            statement,
            sqlx::query_as(statement).bind(ids).fetch_all(&self.pool),
        )
        .await?;
        Ok(names(rows))
    }

    #[tracing::instrument(name = "accounts.directory.search", skip_all, fields(ddd.bounded_context.name = "accounts", input.prefix.bytes = prefix.len()), err)]
    async fn search_names(
        &self,
        prefix: &str,
        ids: Option<&[i64]>,
    ) -> anyhow::Result<Vec<PlayerName>> {
        let pattern = format!(
            "{}%",
            prefix
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let statement = "SELECT id, display_name FROM accounts.accounts WHERE lower(display_name) LIKE lower($1) AND ($2::bigint[] IS NULL OR id=ANY($2)) ORDER BY lower(display_name), id LIMIT 50";
        let rows: Vec<(i64, String)> = database(
            "accounts",
            statement,
            sqlx::query_as(statement)
                .bind(pattern)
                .bind(ids)
                .fetch_all(&self.pool),
        )
        .await?;
        Ok(names(rows))
    }
}

fn names(rows: Vec<(i64, String)>) -> Vec<PlayerName> {
    rows.into_iter()
        .map(|(id, display_name)| PlayerName { id, display_name })
        .collect()
}

impl PlayerDirectory for PostgresPlayerDirectory {
    fn find<'a>(&'a self, ids: &'a [i64]) -> DirectoryLookup<'a> {
        Box::pin(self.find_names(ids))
    }

    fn search<'a>(&'a self, prefix: &'a str, ids: Option<&'a [i64]>) -> DirectoryLookup<'a> {
        Box::pin(self.search_names(prefix, ids))
    }
}
