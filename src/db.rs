use sqlx::postgres::PgArguments;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

type DbBackend = sqlx::Postgres;
type DbArguments = PgArguments;

pub type DbPool = PgPool;

fn bindable_sql(sql: &str) -> &'static str {
    let mut output = String::with_capacity(sql.len());
    let mut index = 0;
    for character in sql.chars() {
        if character == '?' {
            index += 1;
            output.push('$');
            output.push_str(&index.to_string());
        } else {
            output.push(character);
        }
    }
    Box::leak(output.into_boxed_str())
}

pub fn query<'q>(sql: &str) -> sqlx::query::Query<'q, DbBackend, DbArguments> {
    sqlx::query::<DbBackend>(bindable_sql(sql))
}

pub fn query_as<'q, O>(sql: &str) -> sqlx::query::QueryAs<'q, DbBackend, O, DbArguments>
where
    O: for<'r> sqlx::FromRow<'r, <DbBackend as sqlx::Database>::Row>,
{
    sqlx::query_as::<DbBackend, O>(bindable_sql(sql))
}

pub fn query_scalar<'q, O>(sql: &str) -> sqlx::query::QueryScalar<'q, DbBackend, O, DbArguments>
where
    O: for<'r> sqlx::Decode<'r, DbBackend> + sqlx::Type<DbBackend>,
{
    sqlx::query_scalar::<DbBackend, O>(bindable_sql(sql))
}

pub async fn init_db(database_url: &str) -> anyhow::Result<DbPool> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;
    Ok(pool)
}
