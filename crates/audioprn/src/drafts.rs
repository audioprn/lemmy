//! audioprn post drafts: people who can post save the post form (as JSON text, which the website
//! reads back) to finish later. Only the writer sees their drafts.

use crate::is_verified_creator;
use activitypub_federation::config::Data;
use actix_web::web::{Json, Query};
use chrono::{DateTime, Utc};
use diesel::{
  OptionalExtension,
  QueryableByName,
  sql_query,
  sql_types::{Integer, Text, Timestamptz},
};
use diesel_async::RunQueryDsl;
use lemmy_api_utils::{context::LemmyContext, utils::check_local_user_banned_or_deleted};
use lemmy_db_views_local_user::LocalUserView;
use lemmy_diesel_utils::connection::get_conn;
use lemmy_utils::error::{LemmyErrorType, LemmyResult};
use serde::{Deserialize, Serialize};

const MAX_DRAFTS: i32 = 50;
const MAX_TITLE: usize = 300;
/// Room for a full script (200,000 characters) plus the rest of the form.
const MAX_DATA: usize = 300_000;

#[derive(Debug, Serialize, QueryableByName)]
pub struct DraftSummary {
  #[diesel(sql_type = Integer)]
  pub id: i32,
  #[diesel(sql_type = Text)]
  pub title: String,
  #[diesel(sql_type = Timestamptz)]
  pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct DraftsResponse {
  pub drafts: Vec<DraftSummary>,
}

/// GET /post_drafts: the logged-in user's drafts, newest first.
pub async fn list_drafts(
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<DraftsResponse>> {
  let mut pool = context.pool();
  let drafts = sql_query(
    "SELECT id, title, updated_at FROM post_draft WHERE person_id = $1 ORDER BY updated_at DESC",
  )
  .bind::<Integer, _>(local_user_view.person.id.0)
  .load(&mut get_conn(&mut pool).await?)
  .await?;
  Ok(Json(DraftsResponse { drafts }))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DraftId {
  pub id: i32,
}

#[derive(Debug, Serialize, QueryableByName)]
pub struct DraftData {
  #[diesel(sql_type = Text)]
  pub data: String,
}

/// GET /post_draft?id=1: one of the logged-in user's drafts.
pub async fn get_draft(
  Query(data): Query<DraftId>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<DraftData>> {
  let mut pool = context.pool();
  let draft = sql_query("SELECT data FROM post_draft WHERE id = $1 AND person_id = $2")
    .bind::<Integer, _>(data.id)
    .bind::<Integer, _>(local_user_view.person.id.0)
    .get_result(&mut get_conn(&mut pool).await?)
    .await
    .optional()?
    .ok_or(LemmyErrorType::NotFound)?;
  Ok(Json(draft))
}

#[derive(Debug, Deserialize)]
pub struct SaveDraft {
  /// Update this draft; none makes a new one.
  pub id: Option<i32>,
  pub title: String,
  pub data: String,
}

#[derive(Debug, QueryableByName)]
struct Id {
  #[diesel(sql_type = Integer)]
  id: i32,
}

/// PUT /post_draft: save a new draft or update one. Returns its id.
pub async fn save_draft(
  Json(data): Json<SaveDraft>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<DraftId>> {
  check_local_user_banned_or_deleted(&local_user_view)?;
  let me = local_user_view.person.id;
  if !local_user_view.local_user.admin && !is_verified_creator(&mut context.pool(), me).await? {
    return Err(LemmyErrorType::Unknown("only_verified_creators_can_post".into()).into());
  }
  let title = data.title.trim();
  if title.chars().count() > MAX_TITLE || data.data.len() > MAX_DATA {
    return Err(LemmyErrorType::Unknown("invalid_draft".into()).into());
  }
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  // Updates only the user's own draft; a new one only while they have fewer than 50.
  let id = match data.id {
    Some(id) => sql_query(
      "UPDATE post_draft SET title = $3, data = $4, updated_at = now()
       WHERE id = $1 AND person_id = $2 RETURNING id",
    )
    .bind::<Integer, _>(id),
    None => sql_query(
      "INSERT INTO post_draft (person_id, title, data)
       SELECT $2, $3, $4 WHERE (SELECT count(*) FROM post_draft WHERE person_id = $2) < $1
       RETURNING id",
    )
    .bind::<Integer, _>(MAX_DRAFTS),
  }
  .bind::<Integer, _>(me.0)
  .bind::<Text, _>(title)
  .bind::<Text, _>(&data.data)
  .get_result::<Id>(conn)
  .await
  .optional()?
  .map(|r| r.id);
  let Some(id) = id else {
    return Err(
      match data.id {
        Some(_) => LemmyErrorType::NotFound,
        None => LemmyErrorType::Unknown("too_many_drafts".into()),
      }
      .into(),
    );
  };
  Ok(Json(DraftId { id }))
}

/// DELETE /post_draft: delete one of the logged-in user's drafts.
pub async fn delete_draft(
  Json(data): Json<DraftId>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<DraftId>> {
  let mut pool = context.pool();
  sql_query("DELETE FROM post_draft WHERE id = $1 AND person_id = $2")
    .bind::<Integer, _>(data.id)
    .bind::<Integer, _>(local_user_view.person.id.0)
    .execute(&mut get_conn(&mut pool).await?)
    .await?;
  Ok(Json(data))
}
