//! audioprn news flashes. Site news (from admins) shows at the top of the Content and My audioprn
//! left column. Community news is posted by the community's mods on the community page; admins
//! choose which community news also shows in the site news. Verified creators post their own news
//! on their profile. My audioprn shows the site news plus news from the creators and communities
//! the user follows. Up to 300 characters, where every link counts as 23 (links show shortened).

use activitypub_federation::config::Data;
use actix_web::web::{Json, Query};
use chrono::{DateTime, Utc};
use diesel::{
  BoolExpressionMethods,
  ExpressionMethods,
  OptionalExtension,
  QueryDsl,
  dsl::{insert_into, sql},
  sql_types::{Bool, Integer},
};
use diesel_async::RunQueryDsl;
use lemmy_api_utils::{
  context::LemmyContext,
  utils::{is_admin, is_mod_or_admin},
};
use lemmy_db_schema_file::{
  PersonId,
  newtypes::CommunityId,
  schema::{community, person},
};
use lemmy_db_views_local_user::LocalUserView;
use lemmy_diesel_utils::connection::{DbPool, get_conn};
use lemmy_utils::error::{LemmyErrorType, LemmyResult};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::LazyLock};

diesel::table! {
  news_flash (id) {
    id -> Int4,
    body -> Text,
    published_at -> Timestamptz,
    community_id -> Nullable<Int4>,
    in_main -> Bool,
    person_id -> Nullable<Int4>,
  }
}

const MAX_LENGTH: usize = 300;
const LINK_LENGTH: usize = 23;
const SHOWN: i64 = 10;
static LINK: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"https?://\S+").expect("compile regex"));

/// The length that counts: every link is 23 characters, however long it is.
fn counted_length(body: &str) -> usize {
  let links = LINK.find_iter(body).count();
  LINK.replace_all(body, "").chars().count() + links * LINK_LENGTH
}

#[derive(Debug, Serialize, diesel::Queryable)]
pub struct NewsFlash {
  pub id: i32,
  pub body: String,
  pub published_at: DateTime<Utc>,
  pub community_id: Option<i32>,
  /// A community flash an admin also shows in the site news.
  pub in_main: bool,
  /// The creator, for a creator's own news.
  pub person_id: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct NewsFlashView {
  #[serde(flatten)]
  pub news_flash: NewsFlash,
  /// The community's name (title), for community flashes.
  pub community_title: Option<String>,
  /// The creator's display name (or name), for creator flashes.
  pub creator_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NewsResponse {
  pub news: Vec<NewsFlashView>,
  /// For a creator's news: the logged-in user may post it (it's their own, and they're a creator).
  pub can_post: bool,
}

#[derive(Debug, Deserialize)]
pub struct ListNews {
  /// A community's news.
  pub community_id: Option<i32>,
  /// A creator's own news.
  pub person_id: Option<i32>,
  /// My audioprn: the site news plus news from followed creators and communities.
  #[serde(default)]
  pub my: bool,
}

/// GET /news: the latest news flashes, newest first. Without community_id, person_id or my: the
/// site news (admins' flashes and community flashes shown in the main feed).
pub async fn list_news(
  Query(data): Query<ListNews>,
  context: Data<LemmyContext>,
  local_user_view: Option<LocalUserView>,
) -> LemmyResult<Json<NewsResponse>> {
  let me = local_user_view.as_ref().map(|u| u.person.id.0);
  let mut can_post = false;
  if let Some(id) = data.person_id {
    crate::blocks::check_not_blocked_by(
      &mut context.pool(),
      local_user_view.as_ref(),
      PersonId(id),
    )
    .await?;
    if let Some(u) = local_user_view.as_ref().filter(|u| u.person.id.0 == id) {
      can_post = check_can_post_own(&mut context.pool(), u).await.is_ok();
    }
  }
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let query = news_flash::table
    .order_by(news_flash::published_at.desc())
    .limit(SHOWN)
    .into_boxed();
  let site = news_flash::person_id.is_null().and(
    news_flash::community_id
      .is_null()
      .or(news_flash::in_main.eq(true)),
  );
  let flashes: Vec<NewsFlash> = match (data.community_id, data.person_id, me) {
    (Some(id), _, _) => query.filter(news_flash::community_id.eq(id)),
    (_, Some(id), _) => query.filter(news_flash::person_id.eq(id)),
    (_, _, Some(me)) if data.my => query.filter(
      site.or(
        sql::<Bool>(
          "news_flash.community_id IN (SELECT community_id FROM community_actions
             WHERE followed_at IS NOT NULL AND person_id = ",
        )
        .bind::<Integer, _>(me)
        .sql(
          ") OR news_flash.person_id IN (SELECT target_id FROM person_actions
             WHERE followed_at IS NOT NULL AND person_id = ",
        )
        .bind::<Integer, _>(me)
        .sql(")"),
      ),
    ),
    _ => query.filter(site),
  }
  .load(conn)
  .await?;
  // Community titles, loaded separately (see the note in lib.rs).
  let ids: Vec<i32> = flashes.iter().filter_map(|f| f.community_id).collect();
  let titles: HashMap<i32, String> = community::table
    .filter(community::id.eq_any(ids))
    .select((community::id, community::name, community::title))
    .load::<(i32, String, Option<String>)>(conn)
    .await?
    .into_iter()
    .map(|(id, name, title)| (id, title.unwrap_or(name)))
    .collect();
  let ids: Vec<i32> = flashes.iter().filter_map(|f| f.person_id).collect();
  let names: HashMap<i32, String> = person::table
    .filter(person::id.eq_any(ids))
    .select((person::id, person::name, person::display_name))
    .load::<(i32, String, Option<String>)>(conn)
    .await?
    .into_iter()
    .map(|(id, name, display_name)| (id, display_name.unwrap_or(name)))
    .collect();
  let news = flashes
    .into_iter()
    .map(|news_flash| NewsFlashView {
      community_title: news_flash
        .community_id
        .and_then(|id| titles.get(&id).cloned()),
      creator_name: news_flash.person_id.and_then(|id| names.get(&id).cloned()),
      news_flash,
    })
    .collect();
  Ok(Json(NewsResponse { news, can_post }))
}

/// Verified creators (and admins) post their own news.
async fn check_can_post_own(
  pool: &mut DbPool<'_>,
  local_user_view: &LocalUserView,
) -> LemmyResult<()> {
  if is_admin(local_user_view).is_ok()
    || crate::is_verified_creator(pool, local_user_view.person.id).await?
  {
    Ok(())
  } else {
    Err(LemmyErrorType::NotAModOrAdmin.into())
  }
}

/// Admins may post site news and manage everything; a community's mods its community news; a
/// creator their own news.
async fn check_can_manage(
  pool: &mut DbPool<'_>,
  local_user_view: &LocalUserView,
  community_id: Option<i32>,
  person_id: Option<i32>,
) -> LemmyResult<()> {
  match (community_id, person_id) {
    (Some(id), _) => is_mod_or_admin(pool, local_user_view, CommunityId(id)).await,
    (_, Some(id)) if id == local_user_view.person.id.0 => {
      check_can_post_own(pool, local_user_view).await
    }
    _ => is_admin(local_user_view),
  }
}

#[derive(Debug, Deserialize)]
pub struct CreateNewsFlash {
  pub body: String,
  pub community_id: Option<i32>,
  /// The creator's own news (on their profile).
  #[serde(default)]
  pub mine: bool,
}

/// POST /news: admins post site news, a community's mods its community news, creators their own.
pub async fn create_news(
  Json(data): Json<CreateNewsFlash>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<NewsFlash>> {
  let person_id = data.mine.then_some(local_user_view.person.id.0);
  let community_id = data.community_id.filter(|_| !data.mine);
  check_can_manage(&mut context.pool(), &local_user_view, community_id, person_id).await?;
  let body = data.body.trim();
  if body.is_empty() || counted_length(body) > MAX_LENGTH {
    return Err(LemmyErrorType::Unknown("invalid_news_flash".into()).into());
  }
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  Ok(Json(
    insert_into(news_flash::table)
      .values((
        news_flash::body.eq(body),
        news_flash::community_id.eq(community_id),
        news_flash::person_id.eq(person_id),
      ))
      .get_result(conn)
      .await?,
  ))
}

/// The community and creator of a news flash (neither for site news).
async fn flash_owner(pool: &mut DbPool<'_>, id: i32) -> LemmyResult<(Option<i32>, Option<i32>)> {
  let conn = &mut get_conn(pool).await?;
  news_flash::table
    .find(id)
    .select((news_flash::community_id, news_flash::person_id))
    .first(conn)
    .await
    .optional()?
    .ok_or(LemmyErrorType::NotFound.into())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteNewsFlash {
  pub id: i32,
}

/// DELETE /news: admins, the flash's community mods, or its creator remove a news flash.
pub async fn delete_news(
  Json(data): Json<DeleteNewsFlash>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<DeleteNewsFlash>> {
  let (community_id, person_id) = flash_owner(&mut context.pool(), data.id).await?;
  check_can_manage(&mut context.pool(), &local_user_view, community_id, person_id).await?;
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  diesel::delete(news_flash::table.find(data.id))
    .execute(conn)
    .await?;
  Ok(Json(data))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetNewsInMain {
  pub id: i32,
  pub in_main: bool,
}

/// PUT /news/main (admins): show a community news flash in the site news, or not.
pub async fn set_news_in_main(
  Json(data): Json<SetNewsInMain>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<SetNewsInMain>> {
  is_admin(&local_user_view)?;
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  diesel::update(news_flash::table.find(data.id))
    .set(news_flash::in_main.eq(data.in_main))
    .execute(conn)
    .await?;
  Ok(Json(data))
}
