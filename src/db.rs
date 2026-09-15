#[cfg(not(test))]
use sqlx::postgres::PgArguments;
#[cfg(not(test))]
use sqlx::postgres::PgPoolOptions;
#[cfg(test)]
use sqlx::sqlite::SqliteArguments;
#[cfg(test)]
use sqlx::sqlite::SqlitePoolOptions;
#[cfg(not(test))]
use sqlx::PgPool;
#[cfg(test)]
use sqlx::SqlitePool;
#[cfg(not(test))]
type DbBackend = sqlx::Postgres;
#[cfg(test)]
type DbBackend = sqlx::Sqlite;
#[cfg(not(test))]
type DbArguments<'q> = PgArguments;
#[cfg(test)]
type DbArguments<'q> = SqliteArguments<'q>;

#[cfg(not(test))]
pub type DbPool = PgPool;
#[cfg(test)]
pub type DbPool = SqlitePool;

#[cfg(not(test))]
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

#[cfg(test)]
fn bindable_sql(sql: &str) -> &'static str {
    Box::leak(sql.to_owned().into_boxed_str())
}

pub fn query<'q>(sql: &str) -> sqlx::query::Query<'q, DbBackend, DbArguments<'q>> {
    let query = sqlx::query::<DbBackend>(bindable_sql(sql));
    unsafe { std::mem::transmute(query) }
}

pub fn query_as<'q, O>(sql: &str) -> sqlx::query::QueryAs<'q, DbBackend, O, DbArguments<'q>>
where
    O: for<'r> sqlx::FromRow<'r, <DbBackend as sqlx::Database>::Row>,
{
    let query = sqlx::query_as::<DbBackend, O>(bindable_sql(sql));
    unsafe { std::mem::transmute(query) }
}

pub fn query_scalar<'q, O>(sql: &str) -> sqlx::query::QueryScalar<'q, DbBackend, O, DbArguments<'q>>
where
    O: for<'r> sqlx::Decode<'r, DbBackend> + sqlx::Type<DbBackend>,
{
    let query = sqlx::query_scalar::<DbBackend, O>(bindable_sql(sql));
    unsafe { std::mem::transmute(query) }
}

pub async fn init_db(database_url: &str) -> anyhow::Result<DbPool> {
    #[cfg(not(test))]
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;
    #[cfg(test)]
    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;
    Ok(pool)
}
