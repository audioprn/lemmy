//! audioprn Reddit imports: checks and bookkeeping for posts imported from a creator's Reddit
//! data export (the handler is `lemmy_api_crud::post::reddit_import`, as it creates the post).

use activitypub_federation::config::Data;
use actix_web::web::Json;
use chrono::{DateTime, Datelike, Utc};
use diesel::{
  QueryableByName,
  sql_query,
  sql_types::{Array, BigInt, Bool, Integer, Text, Timestamptz},
};
use diesel_async::RunQueryDsl;
use lemmy_api_utils::context::LemmyContext;
use lemmy_db_schema_file::{PersonId, newtypes::PostId};
use lemmy_db_views_local_user::LocalUserView;
use lemmy_diesel_utils::connection::get_conn;
use lemmy_utils::error::{LemmyErrorType, LemmyResult};
use serde::Serialize;

const MAX_IMPORTS_PER_DAY: i64 = 500;

#[derive(QueryableByName)]
struct ImportState {
  #[diesel(sql_type = Bool)]
  done: bool,
  #[diesel(sql_type = BigInt)]
  today: i64,
}

/// Before importing: valid Reddit ids (the post's and its cross-posts') and date, not imported
/// yet, and under the daily limit.
pub async fn check_import(
  context: &LemmyContext,
  person_id: PersonId,
  reddit_id: &str,
  also_reddit_ids: &[String],
  date: DateTime<Utc>,
) -> LemmyResult<()> {
  let valid =
    |id: &str| !id.is_empty() && id.len() <= 12 && id.chars().all(|c| c.is_ascii_alphanumeric());
  if !valid(reddit_id)
    || also_reddit_ids.len() > 20
    || !also_reddit_ids.iter().all(|id| valid(id))
    // At least a day old, so imports never fill the top of the Content page.
    || date > Utc::now() - chrono::Duration::days(1)
    || date.year() < 2005
  {
    return Err(LemmyErrorType::Unknown("invalid_reddit_import".into()).into());
  }
  let mut pool = context.pool();
  let state: ImportState = sql_query(
    "SELECT EXISTS (SELECT 1 FROM reddit_import WHERE person_id = $1 AND reddit_id = $2) AS done,
       (SELECT count(DISTINCT post_id) FROM reddit_import WHERE person_id = $1
         AND imported_at > now() - interval '1 day') AS today",
  )
  .bind::<Integer, _>(person_id.0)
  .bind::<Text, _>(reddit_id)
  .get_result(&mut get_conn(&mut pool).await?)
  .await?;
  if state.done {
    Err(LemmyErrorType::Unknown("already_imported".into()).into())
  } else if state.today >= MAX_IMPORTS_PER_DAY {
    Err(LemmyErrorType::Unknown("import_limit".into()).into())
  } else {
    Ok(())
  }
}

/// After the post is created: give it its Reddit date and remember its Reddit ids.
pub async fn finish_import(
  context: &LemmyContext,
  person_id: PersonId,
  reddit_id: &str,
  also_reddit_ids: &[String],
  post_id: PostId,
  date: DateTime<Utc>,
) -> LemmyResult<()> {
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  sql_query(
    "UPDATE post SET published_at = $2, newest_comment_time_at = $2,
       newest_comment_time_necro_at = $2 WHERE id = $1",
  )
  .bind::<Integer, _>(post_id.0)
  .bind::<Timestamptz, _>(date)
  .execute(conn)
  .await?;
  // Profile pages sort by their own copy of the date.
  sql_query("UPDATE person_content_combined SET published_at = $2 WHERE post_id = $1")
    .bind::<Integer, _>(post_id.0)
    .bind::<Timestamptz, _>(date)
    .execute(conn)
    .await?;
  sql_query("INSERT INTO reddit_import (person_id, reddit_id, post_id) VALUES ($1, $2, $3)")
    .bind::<Integer, _>(person_id.0)
    .bind::<Text, _>(reddit_id)
    .bind::<Integer, _>(post_id.0)
    .execute(conn)
    .await?;
  // Cross-posts are the same post: not offered again, and fills linking them still count.
  sql_query(
    "INSERT INTO reddit_import (person_id, reddit_id, post_id)
     SELECT $1, unnest($2::text[]), $3 ON CONFLICT DO NOTHING",
  )
  .bind::<Integer, _>(person_id.0)
  .bind::<Array<Text>, _>(also_reddit_ids)
  .bind::<Integer, _>(post_id.0)
  .execute(conn)
  .await?;
  Ok(())
}

#[derive(QueryableByName)]
struct RedditId {
  #[diesel(sql_type = Text)]
  reddit_id: String,
}

#[derive(Debug, Serialize)]
pub struct RedditImportsResponse {
  pub reddit_ids: Vec<String>,
}

/// GET /reddit_imports: the Reddit ids the logged-in user has imported.
pub async fn list_imports(
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<RedditImportsResponse>> {
  let mut pool = context.pool();
  let reddit_ids = sql_query("SELECT reddit_id FROM reddit_import WHERE person_id = $1")
    .bind::<Integer, _>(local_user_view.person.id.0)
    .load::<RedditId>(&mut get_conn(&mut pool).await?)
    .await?
    .into_iter()
    .map(|r| r.reddit_id)
    .collect();
  Ok(Json(RedditImportsResponse { reddit_ids }))
}
