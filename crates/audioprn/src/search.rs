//! audioprn search extras (the search itself is `search_sql` in lemmy_db_views_post): popular
//! tags for the chips above the Content page, the user's saved search, and which Script Offer a
//! fill is for (see the audioprn_search migration).

use activitypub_federation::config::Data;
use actix_web::web::{Json, Query};
use diesel::{
  OptionalExtension,
  QueryableByName,
  sql_query,
  sql_types::{BigInt, Bool, Integer, Nullable, Text},
};
use diesel_async::RunQueryDsl;
use lemmy_api_utils::{context::LemmyContext, utils::check_local_user_banned_or_deleted};
use lemmy_db_views_local_user::LocalUserView;
use lemmy_diesel_utils::connection::get_conn;
use lemmy_utils::error::{LemmyErrorType, LemmyResult};
use serde::{Deserialize, Serialize};

const MAX_SAVED_QUERY: usize = 1000;

#[derive(Debug, Serialize, QueryableByName)]
pub struct PopularTag {
  #[diesel(sql_type = Text)]
  pub tag: String,
  #[diesel(sql_type = BigInt)]
  pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct PopularTags {
  pub tags: Vec<PopularTag>,
}

/// GET /popular_tags: the 30 most used title tags on the Content page (lengths left out).
pub async fn get_popular_tags(context: Data<LemmyContext>) -> LemmyResult<Json<PopularTags>> {
  let mut pool = context.pool();
  // Counted hourly by the scheduler (see the audioprn_faster_at_scale migration).
  let tags = sql_query("SELECT tag, count FROM audioprn_popular_tags ORDER BY count DESC, tag")
  .load(&mut get_conn(&mut pool).await?)
  .await?;
  Ok(Json(PopularTags { tags }))
}

#[derive(Debug, Serialize, Deserialize, QueryableByName)]
pub struct SearchPrefs {
  #[diesel(sql_type = Text)]
  pub saved_query: String,
  #[diesel(sql_type = Bool)]
  pub use_saved: bool,
}

/// GET /search_prefs: the user's saved search.
pub async fn get_search_prefs(
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<SearchPrefs>> {
  let mut pool = context.pool();
  let prefs = sql_query("SELECT saved_query, use_saved FROM search_prefs WHERE person_id = $1")
    .bind::<Integer, _>(local_user_view.person.id.0)
    .get_result(&mut get_conn(&mut pool).await?)
    .await
    .optional()?;
  Ok(Json(prefs.unwrap_or(SearchPrefs {
    saved_query: String::new(),
    use_saved: true,
  })))
}

/// PUT /search_prefs: saves the user's saved search and whether to use it.
pub async fn set_search_prefs(
  Json(data): Json<SearchPrefs>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<SearchPrefs>> {
  check_local_user_banned_or_deleted(&local_user_view)?;
  let prefs = SearchPrefs {
    saved_query: data.saved_query.trim().to_string(),
    use_saved: data.use_saved,
  };
  if prefs.saved_query.chars().count() > MAX_SAVED_QUERY {
    return Err(LemmyErrorType::Unknown("search_too_long".into()).into());
  }
  let mut pool = context.pool();
  sql_query(
    "INSERT INTO search_prefs (person_id, saved_query, use_saved) VALUES ($1, $2, $3)
     ON CONFLICT (person_id) DO UPDATE SET saved_query = $2, use_saved = $3",
  )
  .bind::<Integer, _>(local_user_view.person.id.0)
  .bind::<Text, _>(&prefs.saved_query)
  .bind::<Bool, _>(prefs.use_saved)
  .execute(&mut get_conn(&mut pool).await?)
  .await?;
  Ok(Json(prefs))
}

#[derive(Debug, Deserialize)]
pub struct GetScriptFill {
  pub post_id: i32,
}

#[derive(Debug, Serialize, QueryableByName)]
pub struct ScriptFill {
  /// The Script Offer this post fills, if any.
  #[diesel(sql_type = Nullable<Integer>)]
  pub script_post_id: Option<i32>,
  #[diesel(sql_type = Nullable<Text>)]
  pub script_name: Option<String>,
  /// The filler picked it on the post form (rather than it being found from a link).
  #[diesel(sql_type = Bool)]
  pub chosen: bool,
  /// For a Script Offer: how many posts fill it.
  #[diesel(sql_type = BigInt)]
  pub fills: i64,
}

/// GET /script_fill?post_id=1: the Script Offer a post fills, and its own fill count.
pub async fn get_script_fill(
  Query(data): Query<GetScriptFill>,
  context: Data<LemmyContext>,
) -> LemmyResult<Json<ScriptFill>> {
  let mut pool = context.pool();
  let fill = sql_query(
    "SELECT f.script_post_id, sp.name AS script_name, coalesce(f.chosen, FALSE) AS chosen,
       (SELECT count(*) FROM script_fill x JOIN post xp ON xp.id = x.fill_post_id
        WHERE x.script_post_id = $1 AND NOT xp.deleted AND NOT xp.removed) AS fills
     FROM (SELECT $1 AS id) me
     LEFT JOIN script_fill f ON f.fill_post_id = me.id
     LEFT JOIN post sp ON sp.id = f.script_post_id AND NOT sp.deleted AND NOT sp.removed",
  )
  .bind::<Integer, _>(data.post_id)
  .get_result(&mut get_conn(&mut pool).await?)
  .await?;
  Ok(Json(fill))
}

#[derive(Debug, Deserialize)]
pub struct SetScriptFill {
  pub post_id: i32,
  /// None: no chosen script (one may still be found from a link).
  pub script_post_id: Option<i32>,
}

/// PUT /script_fill: the post's creator picks the Script Offer it fills.
pub async fn set_script_fill(
  Json(data): Json<SetScriptFill>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<ScriptFill>> {
  check_local_user_banned_or_deleted(&local_user_view)?;
  {
    let mut pool = context.pool();
    let conn = &mut get_conn(&mut pool).await?;
    let mine = sql_query("SELECT id AS post_id FROM post WHERE id = $1 AND creator_id = $2")
      .bind::<Integer, _>(data.post_id)
      .bind::<Integer, _>(local_user_view.person.id.0)
      .execute(&mut *conn)
      .await?;
    if mine == 0 {
      return Err(LemmyErrorType::Unknown("not_your_post".into()).into());
    }
    match data.script_post_id {
      Some(script) => {
        let set = sql_query(
          "INSERT INTO script_fill (fill_post_id, script_post_id, chosen)
           SELECT $1, $2, TRUE WHERE $1 <> $2 AND audioprn_is_script_offer($2)
             AND EXISTS (SELECT 1 FROM post WHERE id = $2 AND NOT deleted AND NOT removed)
           ON CONFLICT (fill_post_id) DO UPDATE SET script_post_id = $2, chosen = TRUE",
        )
        .bind::<Integer, _>(data.post_id)
        .bind::<Integer, _>(script)
        .execute(&mut *conn)
        .await?;
        if set == 0 {
          return Err(LemmyErrorType::Unknown("not_a_script_offer".into()).into());
        }
      }
      None => {
        sql_query("DELETE FROM script_fill WHERE fill_post_id = $1 AND chosen")
          .bind::<Integer, _>(data.post_id)
          .execute(&mut *conn)
          .await?;
        sql_query("SELECT audioprn_detect_fill($1)")
          .bind::<Integer, _>(data.post_id)
          .execute(&mut *conn)
          .await?;
      }
    }
  }
  get_script_fill(Query(GetScriptFill { post_id: data.post_id }), context).await
}
