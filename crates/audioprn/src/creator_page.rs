//! audioprn creator page extras: the pinned post at the top (social links, terms of use,
//! boundaries, masterlists), and the creator's private stats.

use crate::{clean_link, is_verified_creator};
use activitypub_federation::config::Data;
use actix_web::web::{Json, Query};
use diesel::{
  OptionalExtension,
  QueryableByName,
  sql_query,
  sql_types::{Array, BigInt, Integer, Text},
};
use diesel_async::RunQueryDsl;
use lemmy_api_utils::{context::LemmyContext, utils::check_local_user_banned_or_deleted};
use lemmy_db_schema_file::PersonId;
use lemmy_db_views_local_user::LocalUserView;
use lemmy_diesel_utils::connection::get_conn;
use lemmy_utils::error::{LemmyErrorType, LemmyResult};
use serde::{Deserialize, Serialize};

const MAX_PIN_LINKS: usize = 20;
const MAX_PIN_TEXT: usize = 5000;

#[derive(Debug, Serialize, Deserialize, QueryableByName)]
pub struct CreatorPin {
  #[diesel(sql_type = Array<Text>)]
  pub social_links: Vec<String>,
  #[diesel(sql_type = Text)]
  pub terms: String,
  #[diesel(sql_type = Text)]
  pub boundaries: String,
  #[diesel(sql_type = Text)]
  pub masterlists: String,
}

#[derive(Debug, Serialize)]
pub struct CreatorPinResponse {
  pub pin: Option<CreatorPin>,
  /// The person is a verified creator (so they can write one).
  pub is_creator: bool,
}

#[derive(Debug, Deserialize)]
pub struct GetCreatorPin {
  pub person_id: i32,
}

/// GET /creator_pin?person_id=1: a creator's pinned post (none if they haven't written one).
pub async fn get_creator_pin(
  Query(data): Query<GetCreatorPin>,
  context: Data<LemmyContext>,
) -> LemmyResult<Json<CreatorPinResponse>> {
  let is_creator = is_verified_creator(&mut context.pool(), PersonId(data.person_id)).await?;
  let mut pool = context.pool();
  // Not shown once the creator's verification is revoked.
  let pin = sql_query(
    "SELECT social_links, terms, boundaries, masterlists FROM creator_pin
     WHERE person_id = $1 AND $2",
  )
  .bind::<Integer, _>(data.person_id)
  .bind::<diesel::sql_types::Bool, _>(is_creator)
  .get_result(&mut get_conn(&mut pool).await?)
  .await
  .optional()?;
  Ok(Json(CreatorPinResponse { pin, is_creator }))
}

/// PUT /creator_pin: a verified creator writes their pinned post (all empty removes it).
pub async fn set_creator_pin(
  Json(data): Json<CreatorPin>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<CreatorPinResponse>> {
  check_local_user_banned_or_deleted(&local_user_view)?;
  let me = local_user_view.person.id;
  if !is_verified_creator(&mut context.pool(), me).await? {
    return Err(LemmyErrorType::Unknown("only_verified_creators_can_post".into()).into());
  }
  let pin = CreatorPin {
    social_links: data
      .social_links
      .iter()
      .filter(|l| !l.trim().is_empty())
      .map(|l| clean_link(l))
      .collect::<LemmyResult<_>>()?,
    terms: data.terms.trim().to_string(),
    boundaries: data.boundaries.trim().to_string(),
    masterlists: data.masterlists.trim().to_string(),
  };
  if pin.social_links.len() > MAX_PIN_LINKS
    || [&pin.terms, &pin.boundaries, &pin.masterlists]
      .iter()
      .any(|t| t.chars().count() > MAX_PIN_TEXT)
  {
    return Err(LemmyErrorType::Unknown("invalid_creator_pin".into()).into());
  }
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  if pin.social_links.is_empty()
    && pin.terms.is_empty()
    && pin.boundaries.is_empty()
    && pin.masterlists.is_empty()
  {
    sql_query("DELETE FROM creator_pin WHERE person_id = $1")
      .bind::<Integer, _>(me.0)
      .execute(conn)
      .await?;
    return Ok(Json(CreatorPinResponse {
      pin: None,
      is_creator: true,
    }));
  }
  sql_query(
    "INSERT INTO creator_pin (person_id, social_links, terms, boundaries, masterlists)
     VALUES ($1, $2, $3, $4, $5) ON CONFLICT (person_id) DO UPDATE SET social_links = $2,
     terms = $3, boundaries = $4, masterlists = $5, updated_at = now()",
  )
  .bind::<Integer, _>(me.0)
  .bind::<Array<Text>, _>(&pin.social_links)
  .bind::<Text, _>(&pin.terms)
  .bind::<Text, _>(&pin.boundaries)
  .bind::<Text, _>(&pin.masterlists)
  .execute(conn)
  .await?;
  Ok(Json(CreatorPinResponse {
    pin: Some(pin),
    is_creator: true,
  }))
}

/// A creator's totals, only shown to them.
#[derive(Debug, Serialize, QueryableByName)]
pub struct CreatorStats {
  #[diesel(sql_type = BigInt)]
  pub followers: i64,
  #[diesel(sql_type = BigInt)]
  pub posts: i64,
  /// Emoji reactions on their posts.
  #[diesel(sql_type = BigInt)]
  pub reactions: i64,
  /// Comments by others on their posts.
  #[diesel(sql_type = BigInt)]
  pub comments: i64,
  /// Awards given to their posts.
  #[diesel(sql_type = BigInt)]
  pub awards: i64,
}

/// GET /creator_stats: the logged-in creator's own totals (none for non-creators).
pub async fn get_creator_stats(
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<Option<CreatorStats>>> {
  let me = local_user_view.person.id;
  if !is_verified_creator(&mut context.pool(), me).await? {
    return Ok(Json(None));
  }
  let mut pool = context.pool();
  let stats = sql_query(
    "WITH mine AS (SELECT id FROM post WHERE creator_id = $1 AND NOT deleted AND NOT removed)
     SELECT
       (SELECT count(*) FROM person_actions f JOIN person p ON p.id = f.person_id
         WHERE f.target_id = $1 AND f.followed_at IS NOT NULL AND NOT p.deleted) AS followers,
       (SELECT count(*) FROM mine) AS posts,
       (SELECT count(*) FROM post_reaction WHERE post_id IN (SELECT id FROM mine)) AS reactions,
       (SELECT count(*) FROM comment WHERE post_id IN (SELECT id FROM mine)
         AND creator_id <> $1 AND NOT deleted AND NOT removed) AS comments,
       (SELECT count(*) FROM post_award WHERE post_id IN (SELECT id FROM mine)) AS awards",
  )
  .bind::<Integer, _>(me.0)
  .get_result(&mut get_conn(&mut pool).await?)
  .await?;
  Ok(Json(Some(stats)))
}
