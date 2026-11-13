//! audioprn news flashes. Site news (from admins) shows at the top of the Content and My audioprn
//! left column. Community news is posted by the community's mods on the community page; admins
//! choose which community news also shows in the site news. Up to 300 characters, where every
//! link counts as 23 (links show shortened).

use activitypub_federation::config::Data;
use actix_web::web::{Json, Query};
use chrono::{DateTime, Utc};
use diesel::{
  BoolExpressionMethods,
  ExpressionMethods,
  OptionalExtension,
  QueryDsl,
  dsl::insert_into,
};
use diesel_async::RunQueryDsl;
use lemmy_api_utils::{
  context::LemmyContext,
  utils::{is_admin, is_mod_or_admin},
};
use lemmy_db_schema_file::{newtypes::CommunityId, schema::community};
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
}

#[derive(Debug, Serialize)]
pub struct NewsFlashView {
  #[serde(flatten)]
  pub news_flash: NewsFlash,
  /// The community's name (title), for community flashes.
  pub community_title: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NewsResponse {
  pub news: Vec<NewsFlashView>,
}

#[derive(Debug, Deserialize)]
pub struct ListNews {
  /// A community's news; without it, the site news (admins' flashes and community flashes
  /// shown in the main feed).
  pub community_id: Option<i32>,
}

/// GET /news: the latest news flashes, newest first.
pub async fn list_news(
  Query(data): Query<ListNews>,
  context: Data<LemmyContext>,
) -> LemmyResult<Json<NewsResponse>> {
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let query = news_flash::table
    .order_by(news_flash::published_at.desc())
    .limit(SHOWN)
    .into_boxed();
  let flashes: Vec<NewsFlash> = match data.community_id {
    Some(id) => query.filter(news_flash::community_id.eq(id)),
    None => query.filter(
      news_flash::community_id
        .is_null()
        .or(news_flash::in_main.eq(true)),
    ),
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
  let news = flashes
    .into_iter()
    .map(|news_flash| NewsFlashView {
      community_title: news_flash
        .community_id
        .and_then(|id| titles.get(&id).cloned()),
      news_flash,
    })
    .collect();
  Ok(Json(NewsResponse { news }))
}

/// Admins may post site news and manage everything; a community's mods its community news.
async fn check_can_manage(
  pool: &mut DbPool<'_>,
  local_user_view: &LocalUserView,
  community_id: Option<i32>,
) -> LemmyResult<()> {
  match community_id {
    Some(id) => is_mod_or_admin(pool, local_user_view, CommunityId(id)).await,
    None => is_admin(local_user_view),
  }
}

#[derive(Debug, Deserialize)]
pub struct CreateNewsFlash {
  pub body: String,
  pub community_id: Option<i32>,
}

/// POST /news: admins post site news, a community's mods post its community news.
pub async fn create_news(
  Json(data): Json<CreateNewsFlash>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<NewsFlash>> {
  check_can_manage(&mut context.pool(), &local_user_view, data.community_id).await?;
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
        news_flash::community_id.eq(data.community_id),
      ))
      .get_result(conn)
      .await?,
  ))
}

/// The community of a news flash (none for site news).
async fn flash_community(pool: &mut DbPool<'_>, id: i32) -> LemmyResult<Option<i32>> {
  let conn = &mut get_conn(pool).await?;
  news_flash::table
    .find(id)
    .select(news_flash::community_id)
    .first(conn)
    .await
    .optional()?
    .ok_or(LemmyErrorType::NotFound.into())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteNewsFlash {
  pub id: i32,
}

/// DELETE /news: admins, or the flash's community mods, remove a news flash.
pub async fn delete_news(
  Json(data): Json<DeleteNewsFlash>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<DeleteNewsFlash>> {
  let community_id = flash_community(&mut context.pool(), data.id).await?;
  check_can_manage(&mut context.pool(), &local_user_view, community_id).await?;
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
