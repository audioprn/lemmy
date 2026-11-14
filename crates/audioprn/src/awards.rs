//! audioprn awards. Admins make the award types (the five built-in ones are emoji, new ones an
//! uploaded image) and set how many awards people get (see the audioprn_award_types migration):
//! everyone starts with some, and active people get one more every few days. Awards never expire.
//! One award per person per post (they pick the type), not on their own posts, and it can't be
//! taken back. Awards don't change Engagement.

use crate::reaction::{PostReactions, load};
use activitypub_federation::config::Data;
use actix_web::web::Json;
use diesel::{
  QueryableByName,
  sql_query,
  sql_types::{BigInt, Bool, Integer, Nullable, Text},
};
use diesel_async::{AsyncConnection, RunQueryDsl};
use lemmy_api_utils::{
  context::LemmyContext,
  utils::{check_local_user_banned_or_deleted, is_admin},
};
use lemmy_db_views_local_user::LocalUserView;
use lemmy_diesel_utils::connection::get_conn;
use lemmy_utils::error::{LemmyError, LemmyErrorType, LemmyResult};
use serde::{Deserialize, Serialize};

diesel::table! {
  post_award (post_id, giver_id) {
    post_id -> Int4,
    giver_id -> Int4,
    kind -> Text,
  }
}

/// Uploaded award pictures: a small PNG, JPEG, GIF or WebP as a data URL.
const MAX_IMAGE_LENGTH: usize = 150_000;
const IMAGE_TYPES: [&str; 4] = ["png", "jpeg", "gif", "webp"];

/// The awards a person can give now.
#[derive(Debug, Serialize, QueryableByName)]
pub struct Awards {
  #[diesel(sql_type = BigInt)]
  pub awards_left: i64,
}

const AWARDS_SQL: &str = "SELECT audioprn_awards_left($1) AS awards_left";

/// GET /awards: the logged-in user's awards to give.
pub async fn get_awards(
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<Awards>> {
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  Ok(Json(
    sql_query(AWARDS_SQL)
      .bind::<Integer, _>(local_user_view.person.id.0)
      .get_result(conn)
      .await?,
  ))
}

#[derive(Debug, Deserialize)]
pub struct GivePostAward {
  pub post_id: i32,
  pub kind: String,
}

/// PUT /post_award: give an award to a post. Returns its reactions.
pub async fn give_post_award(
  Json(data): Json<GivePostAward>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<PostReactions>> {
  check_local_user_banned_or_deleted(&local_user_view)?;
  let me = local_user_view.person.id.0;
  {
    let mut pool = context.pool();
    let conn = &mut get_conn(&mut pool).await?;
    conn
      .transaction::<_, LemmyError, _>(async |conn| {
        // One at a time per giver, so parallel requests can't spend the same award twice.
        sql_query("SELECT pg_advisory_xact_lock(hashtext('post_award'), $1)")
          .bind::<Integer, _>(me)
          .execute(conn)
          .await?;
        let awards: Awards = sql_query(AWARDS_SQL)
          .bind::<Integer, _>(me)
          .get_result(conn)
          .await?;
        if awards.awards_left < 1 {
          return Err(LemmyErrorType::Unknown("no_awards_left".into()).into());
        }
        // Only visible posts by someone else, an award that can still be given; once per person
        // per post.
        let added = sql_query(
          "INSERT INTO post_award (post_id, giver_id, kind)
           SELECT id, $2, $3 FROM post
           WHERE id = $1 AND creator_id <> $2 AND NOT deleted AND NOT removed
             AND EXISTS (SELECT 1 FROM award_type WHERE kind = $3 AND NOT hidden)
           ON CONFLICT DO NOTHING",
        )
        .bind::<Integer, _>(data.post_id)
        .bind::<Integer, _>(me)
        .bind::<Text, _>(&data.kind)
        .execute(conn)
        .await?;
        if added == 0 {
          return Err(LemmyErrorType::Unknown("cant_award_post".into()).into());
        }
        Ok(())
      })
      .await?;
  }
  let reactions = load(&context, &[data.post_id], Some(me))
    .await?
    .pop()
    .ok_or(LemmyErrorType::NotFound)?;
  Ok(Json(reactions))
}

#[derive(Debug, Serialize, QueryableByName)]
pub struct AwardType {
  #[diesel(sql_type = Text)]
  pub kind: String,
  #[diesel(sql_type = Text)]
  pub name: String,
  #[diesel(sql_type = Nullable<Text>)]
  pub emoji: Option<String>,
  #[diesel(sql_type = Nullable<Text>)]
  pub image: Option<String>,
  #[diesel(sql_type = Bool)]
  pub hidden: bool,
}

#[derive(Debug, Serialize)]
pub struct AwardTypesResponse {
  pub award_types: Vec<AwardType>,
}

const AWARD_TYPE_COLUMNS: &str = "kind, name, emoji, image, hidden";

/// GET /award_types: every award type, hidden ones too (awards already given still show).
pub async fn get_award_types(context: Data<LemmyContext>) -> LemmyResult<Json<AwardTypesResponse>> {
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let award_types = sql_query(format!(
    "SELECT {AWARD_TYPE_COLUMNS} FROM award_type ORDER BY position"
  ))
  .load(conn)
  .await?;
  Ok(Json(AwardTypesResponse { award_types }))
}

fn check_name(name: &str) -> LemmyResult<&str> {
  let name = name.trim();
  if name.is_empty() || name.chars().count() > 40 {
    return Err(LemmyErrorType::Unknown("invalid_award".into()).into());
  }
  Ok(name)
}

#[derive(Debug, Deserialize)]
pub struct CreateAwardType {
  pub name: String,
  /// The picture as a data URL.
  pub image: String,
}

/// POST /award_type (admins): a new award with an uploaded picture.
pub async fn create_award_type(
  Json(data): Json<CreateAwardType>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<AwardType>> {
  is_admin(&local_user_view)?;
  let name = check_name(&data.name)?;
  let valid_image = data.image.len() <= MAX_IMAGE_LENGTH
    && IMAGE_TYPES
      .iter()
      .any(|t| data.image.starts_with(&format!("data:image/{t};base64,")));
  if !valid_image {
    return Err(LemmyErrorType::Unknown("invalid_award_image".into()).into());
  }
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  Ok(Json(
    sql_query(format!(
      "INSERT INTO award_type (name, image) VALUES ($1, $2) RETURNING {AWARD_TYPE_COLUMNS}"
    ))
    .bind::<Text, _>(name)
    .bind::<Text, _>(&data.image)
    .get_result(conn)
    .await?,
  ))
}

#[derive(Debug, Deserialize)]
pub struct EditAwardType {
  pub kind: String,
  pub name: String,
  pub hidden: bool,
}

/// PUT /award_type (admins): rename an award, or hide it (it can't be given any more).
pub async fn edit_award_type(
  Json(data): Json<EditAwardType>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<AwardType>> {
  is_admin(&local_user_view)?;
  let name = check_name(&data.name)?;
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  Ok(Json(
    sql_query(format!(
      "UPDATE award_type SET name = $2, hidden = $3 WHERE kind = $1
       RETURNING {AWARD_TYPE_COLUMNS}"
    ))
    .bind::<Text, _>(&data.kind)
    .bind::<Text, _>(name)
    .bind::<Bool, _>(data.hidden)
    .get_result(conn)
    .await?,
  ))
}

/// How many awards people get (see the audioprn_award_types migration).
#[derive(Debug, Serialize, Deserialize, QueryableByName)]
pub struct AwardSettings {
  /// Awards everyone starts with.
  #[diesel(sql_type = Integer)]
  pub start_awards: i32,
  /// One more award every this many days, for active people.
  #[diesel(sql_type = Integer)]
  pub every_days: i32,
  /// Active: this many comments, or reactions to this many posts, in the past window_days.
  #[diesel(sql_type = Integer)]
  pub window_days: i32,
  #[diesel(sql_type = Integer)]
  pub min_comments: i32,
  #[diesel(sql_type = Integer)]
  pub min_reactions: i32,
}

const SETTINGS_COLUMNS: &str = "start_awards, every_days, window_days, min_comments, min_reactions";

/// GET /award_settings
pub async fn get_award_settings(context: Data<LemmyContext>) -> LemmyResult<Json<AwardSettings>> {
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  Ok(Json(
    sql_query(format!("SELECT {SETTINGS_COLUMNS} FROM award_settings"))
      .get_result(conn)
      .await?,
  ))
}

/// PUT /award_settings (admins). Applies to everyone's awards, past days included.
pub async fn set_award_settings(
  Json(data): Json<AwardSettings>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<AwardSettings>> {
  is_admin(&local_user_view)?;
  let in_range = |v: i32, min: i32, max: i32| (min..=max).contains(&v);
  if !(in_range(data.start_awards, 0, 1000)
    && in_range(data.every_days, 1, 365)
    && in_range(data.window_days, 1, 365)
    && in_range(data.min_comments, 0, 1000)
    && in_range(data.min_reactions, 0, 1000))
  {
    return Err(LemmyErrorType::Unknown("invalid_award_settings".into()).into());
  }
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  Ok(Json(
    sql_query(format!(
      "UPDATE award_settings SET start_awards = $1, every_days = $2, window_days = $3,
         min_comments = $4, min_reactions = $5 RETURNING {SETTINGS_COLUMNS}"
    ))
    .bind::<Integer, _>(data.start_awards)
    .bind::<Integer, _>(data.every_days)
    .bind::<Integer, _>(data.window_days)
    .bind::<Integer, _>(data.min_comments)
    .bind::<Integer, _>(data.min_reactions)
    .get_result(conn)
    .await?,
  ))
}
