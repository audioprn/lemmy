//! audioprn creator page extras: the pinned post at the top (social links, terms of use,
//! boundaries), up to 4 of their posts pinned under it, and the creator's private stats.

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
    "SELECT social_links, terms, boundaries FROM creator_pin
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
  };
  if pin.social_links.len() > MAX_PIN_LINKS
    || [&pin.terms, &pin.boundaries]
      .iter()
      .any(|t| t.chars().count() > MAX_PIN_TEXT)
  {
    return Err(LemmyErrorType::Unknown("invalid_creator_pin".into()).into());
  }
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  if pin.social_links.is_empty() && pin.terms.is_empty() && pin.boundaries.is_empty() {
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
    "INSERT INTO creator_pin (person_id, social_links, terms, boundaries)
     VALUES ($1, $2, $3, $4) ON CONFLICT (person_id) DO UPDATE SET social_links = $2,
     terms = $3, boundaries = $4, updated_at = now()",
  )
  .bind::<Integer, _>(me.0)
  .bind::<Array<Text>, _>(&pin.social_links)
  .bind::<Text, _>(&pin.terms)
  .bind::<Text, _>(&pin.boundaries)
  .execute(conn)
  .await?;
  Ok(Json(CreatorPinResponse {
    pin: Some(pin),
    is_creator: true,
  }))
}

const MAX_PAGE_PINS: i64 = 4;

#[derive(Debug, QueryableByName)]
struct PostId {
  #[diesel(sql_type = Integer)]
  post_id: i32,
}

#[derive(Debug, Serialize)]
pub struct PagePinsResponse {
  /// Oldest pin first.
  pub post_ids: Vec<i32>,
  /// The person is a verified creator (so they can pin posts).
  pub is_creator: bool,
}

/// GET /creator_page_pins?person_id=1: the posts a creator pinned on their page.
pub async fn get_page_pins(
  Query(data): Query<GetCreatorPin>,
  context: Data<LemmyContext>,
) -> LemmyResult<Json<PagePinsResponse>> {
  let is_creator = is_verified_creator(&mut context.pool(), PersonId(data.person_id)).await?;
  let mut pool = context.pool();
  let post_ids = sql_query(
    "SELECT pp.post_id FROM creator_page_pin pp JOIN post p ON p.id = pp.post_id
     WHERE pp.person_id = $1 AND $2 AND NOT p.deleted AND NOT p.removed
     ORDER BY pp.pinned_at",
  )
  .bind::<Integer, _>(data.person_id)
  .bind::<diesel::sql_types::Bool, _>(is_creator)
  .load::<PostId>(&mut get_conn(&mut pool).await?)
  .await?
  .into_iter()
  .map(|p| p.post_id)
  .collect();
  Ok(Json(PagePinsResponse {
    post_ids,
    is_creator,
  }))
}

#[derive(Debug, Deserialize)]
pub struct SetPagePin {
  pub post_id: i32,
  pub pinned: bool,
}

/// PUT /creator_page_pin: a verified creator pins (up to 4) or unpins one of their own posts.
pub async fn set_page_pin(
  Json(data): Json<SetPagePin>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<PagePinsResponse>> {
  check_local_user_banned_or_deleted(&local_user_view)?;
  let me = local_user_view.person.id;
  if !is_verified_creator(&mut context.pool(), me).await? {
    return Err(LemmyErrorType::Unknown("only_verified_creators_can_post".into()).into());
  }
  {
    let mut pool = context.pool();
    let conn = &mut get_conn(&mut pool).await?;
    set_page_pin_in_db(conn, me.0, &data).await?;
  }
  get_page_pins(Query(GetCreatorPin { person_id: me.0 }), context).await
}

async fn set_page_pin_in_db(
  conn: &mut diesel_async::AsyncPgConnection,
  me: i32,
  data: &SetPagePin,
) -> LemmyResult<()> {
  if data.pinned {
    // Counts only pins of live posts, like the list. Pinning a pinned post does nothing.
    let added = sql_query(
      "INSERT INTO creator_page_pin (person_id, post_id)
       SELECT $1, id FROM post
       WHERE id = $2 AND creator_id = $1 AND NOT deleted AND NOT removed
         AND (SELECT count(*) FROM creator_page_pin pp JOIN post p ON p.id = pp.post_id
              WHERE pp.person_id = $1 AND NOT p.deleted AND NOT p.removed) < $3
       ON CONFLICT DO NOTHING",
    )
    .bind::<Integer, _>(me)
    .bind::<Integer, _>(data.post_id)
    .bind::<BigInt, _>(MAX_PAGE_PINS)
    .execute(&mut *conn)
    .await?;
    let pinned = sql_query(
      "SELECT post_id FROM creator_page_pin WHERE person_id = $1 AND post_id = $2",
    )
    .bind::<Integer, _>(me)
    .bind::<Integer, _>(data.post_id)
    .get_result::<PostId>(&mut *conn)
    .await
    .optional()?
    .is_some();
    if added == 0 && !pinned {
      let mine = sql_query(
        "SELECT id AS post_id FROM post
         WHERE id = $1 AND creator_id = $2 AND NOT deleted AND NOT removed",
      )
      .bind::<Integer, _>(data.post_id)
      .bind::<Integer, _>(me)
      .get_result::<PostId>(&mut *conn)
      .await
      .optional()?
      .is_some();
      let error = if mine { "too_many_page_pins" } else { "not_your_post" };
      return Err(LemmyErrorType::Unknown(error.into()).into());
    }
  } else {
    sql_query("DELETE FROM creator_page_pin WHERE person_id = $1 AND post_id = $2")
      .bind::<Integer, _>(me)
      .bind::<Integer, _>(data.post_id)
      .execute(&mut *conn)
      .await?;
  }
  Ok(())
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
