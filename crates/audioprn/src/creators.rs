//! audioprn creators: the Creators page list, and following creators.
//!
//! Following reuses Lemmy's person follows (`person_actions.followed_at`), which upstream only
//! uses for federation (off on this site).

use crate::is_verified_creator;
use activitypub_federation::config::Data;
use actix_web::web::{Json, Query};
use chrono::{DateTime, Utc};
use diesel::{
  sql_query,
  sql_types::{BigInt, Bool, Integer, Nullable, Text, Timestamptz},
};
use diesel_async::RunQueryDsl;
use lemmy_api_utils::{context::LemmyContext, utils::check_local_user_banned_or_deleted};
use lemmy_db_schema_file::PersonId;
use lemmy_db_views_local_user::LocalUserView;
use lemmy_diesel_utils::connection::get_conn;
use lemmy_utils::error::{LemmyErrorType, LemmyResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, diesel::QueryableByName)]
pub struct Creator {
  #[diesel(sql_type = Integer)]
  pub id: i32,
  #[diesel(sql_type = Text)]
  pub name: String,
  #[diesel(sql_type = Nullable<Text>)]
  pub display_name: Option<String>,
  #[diesel(sql_type = Nullable<Text>)]
  pub avatar: Option<String>,
  #[diesel(sql_type = Nullable<Timestamptz>)]
  pub last_post_at: Option<DateTime<Utc>>,
  /// The logged-in user follows this creator.
  #[diesel(sql_type = Bool)]
  pub following: bool,
  /// Follower count; none when the creator hides it (they always see their own).
  #[diesel(sql_type = Nullable<BigInt>)]
  pub followers: Option<i64>,
  #[diesel(sql_type = Bool)]
  pub hide_follower_count: bool,
}

#[derive(Debug, Deserialize)]
pub struct ListCreators {
  /// Only this creator (empty list if they aren't a verified creator).
  pub person_id: Option<i32>,
  /// `recent` (default), `alpha` or `random`.
  pub sort: Option<String>,
  /// For `random`: the same seed gives the same order, so later pages follow on.
  pub seed: Option<String>,
  /// Pages of 50, from 1.
  pub page: Option<i64>,
}

const CREATORS_PAGE: i64 = 50;

#[derive(Debug, Serialize)]
pub struct ListCreatorsResponse {
  pub creators: Vec<Creator>,
}

/// GET /creators: verified creators who aren't deleted or banned, a page of 50 at a time (sorted
/// here: there can be thousands).
pub async fn list_creators(
  Query(data): Query<ListCreators>,
  context: Data<LemmyContext>,
  local_user_view: Option<LocalUserView>,
) -> LemmyResult<Json<ListCreatorsResponse>> {
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  // Only fixed text goes into the query; the seed is bound.
  let order = match data.sort.as_deref() {
    Some("alpha") => "lower(coalesce(p.display_name, p.name)), p.id",
    Some("random") => "md5(p.id::text || $3), p.id",
    _ => "last_post_at DESC NULLS LAST, p.name",
  };
  let offset = (data.page.unwrap_or(1).max(1) - 1) * CREATORS_PAGE;
  let creators = sql_query(format!(
    "SELECT p.id, p.name, p.display_name, p.avatar,
       (SELECT max(published_at) FROM post
         WHERE creator_id = p.id AND NOT deleted AND NOT removed) AS last_post_at,
       EXISTS (SELECT 1 FROM person_actions f WHERE f.person_id = $1 AND f.target_id = p.id
         AND f.followed_at IS NOT NULL) AS following,
       CASE WHEN NOT a.hide_follower_count OR p.id = $1 THEN
         (SELECT count(*) FROM person_actions f JOIN person fp ON fp.id = f.person_id
           WHERE f.target_id = p.id AND f.followed_at IS NOT NULL AND NOT fp.deleted)
       END AS followers,
       a.hide_follower_count
     FROM creator_application a JOIN person p ON p.id = a.person_id
     WHERE a.approved AND NOT p.deleted AND ($2::int IS NULL OR p.id = $2)
       AND NOT EXISTS (SELECT 1 FROM person_actions b WHERE b.person_id = $1 AND b.target_id = p.id
         AND (b.blocked_at IS NOT NULL OR b.blocked_by_at IS NOT NULL)) -- audioprn_blocked(), as a join
       AND NOT EXISTS (SELECT 1 FROM instance_actions i WHERE i.person_id = p.id
         AND i.received_ban_at IS NOT NULL AND (i.ban_expires_at IS NULL OR i.ban_expires_at > now()))
     ORDER BY {order} LIMIT {CREATORS_PAGE} OFFSET $4",
  ))
  .bind::<Nullable<Integer>, _>(local_user_view.map(|l| l.person.id.0))
  .bind::<Nullable<Integer>, _>(data.person_id)
  .bind::<Text, _>(data.seed.unwrap_or_default())
  .bind::<BigInt, _>(offset)
  .load(conn)
  .await?;
  Ok(Json(ListCreatorsResponse { creators }))
}

#[derive(Debug, Deserialize)]
pub struct FollowCreator {
  pub creator_id: i32,
  pub follow: bool,
}

#[derive(Debug, Serialize)]
pub struct FollowCreatorResponse {
  pub following: bool,
}

/// PUT /creator_follow: follow or unfollow a verified creator.
pub async fn follow_creator(
  Json(data): Json<FollowCreator>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<FollowCreatorResponse>> {
  check_local_user_banned_or_deleted(&local_user_view)?;
  let me = local_user_view.person.id.0;
  if data.follow
    && (data.creator_id == me
      || !is_verified_creator(&mut context.pool(), PersonId(data.creator_id)).await?)
  {
    return Err(LemmyErrorType::NotFound.into());
  }
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  // No following across a block.
  let changed = sql_query(if data.follow {
    "INSERT INTO person_actions (person_id, target_id, followed_at, follow_pending)
     SELECT $1, $2, now(), false WHERE NOT audioprn_blocked($1, $2)
     ON CONFLICT (person_id, target_id)
     DO UPDATE SET followed_at = now(), follow_pending = false"
  } else {
    "UPDATE person_actions SET followed_at = NULL, follow_pending = NULL
     WHERE person_id = $1 AND target_id = $2"
  })
  .bind::<Integer, _>(me)
  .bind::<Integer, _>(data.creator_id)
  .execute(conn)
  .await?;
  Ok(Json(FollowCreatorResponse {
    following: data.follow && changed > 0,
  }))
}

#[derive(Debug, Serialize, diesel::QueryableByName)]
pub struct Follower {
  #[diesel(sql_type = Text)]
  pub name: String,
  #[diesel(sql_type = Nullable<Text>)]
  pub display_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FollowersResponse {
  pub followers: Vec<Follower>,
}

/// GET /creator_followers: the logged-in creator's followers, newest first. Only they see this.
pub async fn my_followers(
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<FollowersResponse>> {
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let followers = sql_query(
    "SELECT p.name, p.display_name FROM person_actions f JOIN person p ON p.id = f.person_id
     WHERE f.target_id = $1 AND f.followed_at IS NOT NULL AND NOT p.deleted
     ORDER BY f.followed_at DESC",
  )
  .bind::<Integer, _>(local_user_view.person.id.0)
  .load(conn)
  .await?;
  Ok(Json(FollowersResponse { followers }))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreatorSettings {
  pub hide_follower_count: bool,
}

/// PUT /creator_settings: a creator shows or hides their public follower count.
pub async fn set_creator_settings(
  Json(data): Json<CreatorSettings>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<CreatorSettings>> {
  use crate::creator_application::dsl::{creator_application, hide_follower_count, person_id};
  use diesel::{ExpressionMethods, QueryDsl};
  check_local_user_banned_or_deleted(&local_user_view)?;
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  diesel::update(creator_application.filter(person_id.eq(local_user_view.person.id.0)))
    .set(hide_follower_count.eq(data.hide_follower_count))
    .execute(conn)
    .await?;
  Ok(Json(data))
}

const MAX_FOOTER_BLURB: usize = 500;
const MAX_FOOTER_LINKS: usize = 10;
const MAX_SUPPORT_TEXT: usize = 500;

/// A creator's optional footer, shown under each of their posts, and cash box (support links,
/// markdown), shown inside their Content posts at the bottom.
#[derive(Debug, Serialize, Deserialize)]
pub struct CreatorFooter {
  #[serde(default)]
  pub person_id: i32,
  pub blurb: String,
  pub links: Vec<String>,
  #[serde(default)]
  pub support: String,
}

#[derive(Debug, Deserialize)]
pub struct GetCreatorFooters {
  /// Comma-separated person ids.
  pub person_ids: String,
}

#[derive(Debug, Serialize)]
pub struct CreatorFootersResponse {
  pub footers: Vec<CreatorFooter>,
}

/// GET /creator_footers?person_ids=1,2: footers of these verified creators (empty ones left out).
pub async fn get_creator_footers(
  Query(data): Query<GetCreatorFooters>,
  context: Data<LemmyContext>,
) -> LemmyResult<Json<CreatorFootersResponse>> {
  use crate::creator_application::dsl as a;
  use diesel::{BoolExpressionMethods, ExpressionMethods, QueryDsl};
  let ids: Vec<i32> = data
    .person_ids
    .split(',')
    .filter_map(|id| id.trim().parse().ok())
    .take(100)
    .collect();
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let footers = a::creator_application
    .filter(a::person_id.eq_any(&ids))
    .filter(a::approved.eq(true))
    .filter(
      a::footer_blurb
        .ne("")
        .or(a::footer_links.ne(Vec::<String>::new()))
        .or(a::support_text.ne("")),
    )
    .select((a::person_id, a::footer_blurb, a::footer_links, a::support_text))
    .load::<(i32, String, Vec<String>, String)>(conn)
    .await?
    .into_iter()
    .map(|(person_id, blurb, links, support)| CreatorFooter {
      person_id,
      blurb,
      links,
      support,
    })
    .collect();
  Ok(Json(CreatorFootersResponse { footers }))
}

/// PUT /creator_footer: a creator sets their footer (blurb up to 500 characters, up to 10 links).
pub async fn set_creator_footer(
  Json(data): Json<CreatorFooter>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<CreatorFooter>> {
  use crate::creator_application::dsl as a;
  use diesel::{ExpressionMethods, QueryDsl};
  check_local_user_banned_or_deleted(&local_user_view)?;
  let blurb = data.blurb.trim().to_string();
  let support = data.support.trim().to_string();
  let links: Vec<String> = data
    .links
    .iter()
    .filter(|l| !l.trim().is_empty())
    .map(|l| crate::clean_link(l))
    .collect::<LemmyResult<_>>()?;
  if blurb.chars().count() > MAX_FOOTER_BLURB
    || links.len() > MAX_FOOTER_LINKS
    || support.chars().count() > MAX_SUPPORT_TEXT
  {
    return Err(LemmyErrorType::Unknown("invalid_creator_footer".into()).into());
  }
  let person_id = local_user_view.person.id.0;
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let updated = diesel::update(
    a::creator_application
      .filter(a::person_id.eq(person_id))
      .filter(a::approved.eq(true)),
  )
  .set((
    a::footer_blurb.eq(&blurb),
    a::footer_links.eq(&links),
    a::support_text.eq(&support),
  ))
  .execute(conn)
  .await?;
  if updated == 0 {
    return Err(LemmyErrorType::Unknown("only_verified_creators_can_post".into()).into());
  }
  Ok(Json(CreatorFooter {
    person_id,
    blurb,
    links,
    support,
  }))
}
