//! audioprn post additions: one flair per post, and credits (writer, editor, voice actor).

use activitypub_federation::config::Data;
use actix_web::web::{Json, Query};
use diesel::{ExpressionMethods, QueryDsl, dsl::insert_into};
use diesel_async::{AsyncConnection, RunQueryDsl};
use lemmy_api_utils::context::LemmyContext;
use lemmy_db_schema_file::{
  newtypes::{CommunityId, CommunityTagId, PostId},
  schema::{community, community_tag, post},
};
use regex::Regex;
use std::sync::LazyLock;
use lemmy_db_views_local_user::LocalUserView;
use lemmy_diesel_utils::connection::{DbPool, get_conn};
use lemmy_utils::error::{LemmyError, LemmyErrorExt, LemmyErrorType, LemmyResult};
use serde::{Deserialize, Serialize};

diesel::table! {
  post_credit (id) {
    id -> Int4,
    post_id -> Int4,
    role -> Text,
    name -> Text,
  }
}

/// A space between tags that touch: "[F4M][Romance]" becomes "[F4M] [Romance]".
pub fn space_tags(title: &str) -> String {
  title.replace("][", "] [")
}

const ROLES: [&str; 3] = ["writer", "editor", "voice_actor"];
const MAX_CREDITS: usize = 30;
const MAX_NAME_LENGTH: usize = 100;
const MAX_POST_IDS: usize = 100;

/// Bracket tags, as read by the content filters (see the audioprn_post_filter_tags migration).
static TAG: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"\[[^\[\]]{1,40}\]").expect("compile regex"));
/// A gender tag: parts of F, M, A, TF, TM, NB joined by 4, e.g. [F4M], [M4F4M], [TF4A], [NB4M].
static GENDER_TAG: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"^\[(?i)((TF|TM|NB|F|M|A)+4)+(TF|TM|NB|F|M|A)+\]$").expect("compile regex")
});
/// Tags other than gender tags.
const MAX_TITLE_TAGS: usize = 7;

/// One voice in a gender tag part: TF, TM and NB are one voice each, like F, M and A.
static VOICE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?i)TF|TM|NB|F|M|A").expect("compile regex"));

/// "collab" or "collaboration"; "self-collab" (one creator doing it all) is captured to be skipped.
static COLLAB: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)\b(self-?)?collab(oration)?s?\b").expect("compile regex")
});

/// More than one voice: a gender tag with two or more voices ([FM4M], [FF4A], and [M4F4M], whose
/// parts before the last are all voices), or "collab" / "collaboration" in the title (not
/// "self-collab"). Post texts are left out: they mostly mention other collabs or the writer.
/// Reddit imports of audios like this come in hidden (scripts are fine), for the creator to unhide
/// once everyone consented.
pub fn is_multi_voice(title: &str) -> bool {
  let voices = |tag: &str| {
    let parts: Vec<&str> = tag
      .trim_matches(['[', ']'])
      .split('4')
      .collect();
    parts[..parts.len() - 1]
      .iter()
      .map(|p| VOICE.find_iter(p).count())
      .sum::<usize>()
  };
  TAG
    .find_iter(title)
    .filter(|t| GENDER_TAG.is_match(t.as_str()))
    .any(|t| voices(t.as_str()) > 1)
    || COLLAB.captures_iter(title).any(|c| c.get(1).is_none())
}

/// Content and community post titles start with a gender tag like [F4M] and have at most 7
/// other tags. Creator page posts and admins' posts are exempt.
pub async fn check_title_tags(
  pool: &mut DbPool<'_>,
  community_id: CommunityId,
  title: &str,
  local_user_view: &LocalUserView,
) -> LemmyResult<()> {
  if local_user_view.local_user.admin {
    return Ok(());
  }
  let conn = &mut get_conn(pool).await?;
  let community_name: String = community::table
    .find(community_id)
    .select(community::name)
    .first(conn)
    .await?;
  if community_name == "creators" {
    return Ok(());
  }
  let title = title.trim_start();
  if !TAG
    .find(title)
    .is_some_and(|t| t.start() == 0 && GENDER_TAG.is_match(t.as_str()))
  {
    return Err(LemmyErrorType::Unknown("title_needs_gender_tag".into()).into());
  }
  if TAG
    .find_iter(title)
    .filter(|t| !GENDER_TAG.is_match(t.as_str()))
    .count()
    > MAX_TITLE_TAGS
  {
    return Err(LemmyErrorType::Unknown("too_many_title_tags".into()).into());
  }
  Ok(())
}

/// Every post needs exactly one flair (community tag), in communities that have flairs.
/// Admins' mod posts are exempt.
pub async fn check_one_flair(
  pool: &mut DbPool<'_>,
  community_id: CommunityId,
  tags: Option<&[CommunityTagId]>,
  local_user_view: &LocalUserView,
) -> LemmyResult<()> {
  if local_user_view.local_user.admin || tags.is_some_and(|t| t.len() == 1) {
    return Ok(());
  }
  let conn = &mut get_conn(pool).await?;
  let has_flairs: bool = diesel::select(diesel::dsl::exists(
    community_tag::table
      .filter(community_tag::community_id.eq(community_id))
      .filter(community_tag::deleted.eq(false)),
  ))
  .get_result(conn)
  .await?;
  if has_flairs {
    Err(LemmyErrorType::Unknown("post_needs_one_flair".into()).into())
  } else {
    Ok(())
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PostCredit {
  pub role: String,
  pub name: String,
}

/// Trims names, drops blank ones, and checks roles and lengths.
fn validate_credits(credits: Vec<PostCredit>) -> LemmyResult<Vec<PostCredit>> {
  let credits: Vec<PostCredit> = credits
    .into_iter()
    .map(|c| PostCredit {
      role: c.role,
      name: c.name.trim().to_string(),
    })
    .filter(|c| !c.name.is_empty())
    .collect();
  let invalid = credits.len() > MAX_CREDITS
    || credits.iter().any(|c| {
      !ROLES.contains(&c.role.as_str())
        || c.name.chars().count() > MAX_NAME_LENGTH
        || c.name.contains('\n')
    });
  if invalid {
    Err(LemmyErrorType::Unknown("invalid_post_credits".into()).into())
  } else {
    Ok(credits)
  }
}

// Posts their creator keeps off the Content page (they still show in their community).
diesel::table! {
  post_off_content (post_id) {
    post_id -> Int4,
    /// Sapphic: "No comments from men are welcome on this post" (shown above its comments).
    no_men -> Bool,
  }
}

#[derive(Debug, Deserialize)]
pub struct SetPostCredits {
  pub post_id: PostId,
  pub credits: Vec<PostCredit>,
  /// Keep the post off the Content page. Unchanged when both this and `no_men` are left out.
  pub off_content: Option<bool>,
  /// No comments from men are welcome (also keeps it off the Content page).
  pub no_men: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct PostCreditsResponse {
  pub credits: Vec<PostCreditView>,
  /// Which of the posts are kept off the Content page.
  pub off_content: Vec<i32>,
  /// Which of the posts welcome no comments from men.
  pub no_men: Vec<i32>,
}

#[derive(Debug, Serialize)]
pub struct PostCreditView {
  pub post_id: PostId,
  pub role: String,
  pub name: String,
}

/// PUT /post_credits: the post's creator (or an admin) replaces its credits.
pub async fn set_post_credits(
  Json(data): Json<SetPostCredits>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<PostCreditsResponse>> {
  lemmy_api_utils::utils::check_local_user_banned_or_deleted(&local_user_view)?;
  let credits = validate_credits(data.credits)?;
  let names: Vec<&str> = credits.iter().map(|c| c.name.as_str()).collect();
  crate::optout::check_opt_out(&mut context.pool(), &[], &names).await?;
  let post_id = data.post_id;
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;

  let creator_id: i32 = post::table
    .find(post_id)
    .select(post::creator_id)
    .first(conn)
    .await
    .with_lemmy_type(LemmyErrorType::NotFound)?;
  if creator_id != local_user_view.person.id.0 && !local_user_view.local_user.admin {
    return Err(LemmyErrorType::NoPostEditAllowed.into());
  }

  let rows: Vec<_> = credits
    .iter()
    .map(|c| {
      (
        post_credit::post_id.eq(post_id.0),
        post_credit::role.eq(&c.role),
        post_credit::name.eq(&c.name),
      )
    })
    .collect();
  conn
    .transaction::<_, LemmyError, _>(async |conn| {
      diesel::delete(post_credit::table.filter(post_credit::post_id.eq(post_id.0)))
        .execute(conn)
        .await?;
      if !rows.is_empty() {
        insert_into(post_credit::table)
          .values(&rows)
          .execute(conn)
          .await?;
      }
      if data.off_content.is_some() || data.no_men.is_some() {
        diesel::delete(post_off_content::table.find(post_id.0))
          .execute(conn)
          .await?;
        let no_men = data.no_men == Some(true);
        if no_men || data.off_content == Some(true) {
          insert_into(post_off_content::table)
            .values((
              post_off_content::post_id.eq(post_id.0),
              post_off_content::no_men.eq(no_men),
            ))
            .execute(conn)
            .await?;
        }
      }
      Ok(())
    })
    .await?;

  let (off_content, no_men) = off_content_ids(conn, &[post_id.0]).await?;
  Ok(Json(PostCreditsResponse {
    off_content,
    no_men,
    credits: credits
      .into_iter()
      .map(|c| PostCreditView {
        post_id,
        role: c.role,
        name: c.name,
      })
      .collect(),
  }))
}

#[derive(Debug, Deserialize)]
pub struct GetPostCredits {
  /// Comma-separated post ids.
  pub post_ids: String,
}

/// GET /post_credits?post_ids=1,2,3: credits for a page of posts, in the order they were entered.
pub async fn get_post_credits(
  Query(data): Query<GetPostCredits>,
  context: Data<LemmyContext>,
) -> LemmyResult<Json<PostCreditsResponse>> {
  let ids: Vec<i32> = data
    .post_ids
    .split(',')
    .filter_map(|id| id.trim().parse().ok())
    .take(MAX_POST_IDS)
    .collect();
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let credits = post_credit::table
    .filter(post_credit::post_id.eq_any(&ids))
    .order_by(post_credit::id)
    .select((post_credit::post_id, post_credit::role, post_credit::name))
    .load::<(i32, String, String)>(conn)
    .await?
    .into_iter()
    .map(|(post_id, role, name)| PostCreditView {
      post_id: PostId(post_id),
      role,
      name,
    })
    .collect();
  let (off_content, no_men) = off_content_ids(conn, &ids).await?;
  Ok(Json(PostCreditsResponse {
    credits,
    off_content,
    no_men,
  }))
}

/// Which of the posts are kept off the Content page, and which of those welcome no men.
async fn off_content_ids(
  conn: &mut diesel_async::AsyncPgConnection,
  ids: &[i32],
) -> LemmyResult<(Vec<i32>, Vec<i32>)> {
  let rows: Vec<(i32, bool)> = post_off_content::table
    .filter(post_off_content::post_id.eq_any(ids))
    .select((post_off_content::post_id, post_off_content::no_men))
    .load(conn)
    .await?;
  let no_men = rows.iter().filter(|r| r.1).map(|r| r.0).collect();
  Ok((rows.into_iter().map(|r| r.0).collect(), no_men))
}

#[derive(Debug, Serialize, diesel::QueryableByName)]
pub struct CreditedPost {
  #[diesel(sql_type = diesel::sql_types::Integer)]
  pub id: i32,
  #[diesel(sql_type = diesel::sql_types::Text)]
  pub name: String,
  #[diesel(sql_type = diesel::sql_types::Text)]
  pub creator_name: String,
  /// The credited person hid it from their profile (only they get hidden ones).
  #[diesel(sql_type = diesel::sql_types::Bool)]
  pub hidden: bool,
}

#[derive(Debug, Deserialize)]
pub struct GetCreditedPosts {
  pub person_id: i32,
}

#[derive(Debug, Serialize)]
pub struct CreditedPostsResponse {
  pub posts: Vec<CreditedPost>,
}

/// GET /credited_posts?person_id=1: other people's posts that credit this person as u/name,
/// newest first (shown on their profile). Posts they hid show only to them.
pub async fn get_credited_posts(
  Query(data): Query<GetCreditedPosts>,
  context: Data<LemmyContext>,
  local_user_view: Option<LocalUserView>,
) -> LemmyResult<Json<CreditedPostsResponse>> {
  let me = local_user_view.map(|u| u.person.id.0);
  let is_me = me == Some(data.person_id);
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let posts = diesel::sql_query(
    "SELECT p.id, p.name, pe.name AS creator_name, h.post_id IS NOT NULL AS hidden FROM post p
     JOIN person pe ON pe.id = p.creator_id
     LEFT JOIN credited_post_hidden h ON h.person_id = $1 AND h.post_id = p.id
     WHERE NOT p.deleted AND NOT p.removed AND p.creator_id <> $1
       AND ($2 OR h.post_id IS NULL) AND NOT audioprn_blocked($3, p.creator_id)
       AND EXISTS (SELECT 1 FROM post_credit pc WHERE pc.post_id = p.id
         AND lower(pc.name) = 'u/' || lower((SELECT name FROM person WHERE id = $1)))
     ORDER BY p.published_at DESC LIMIT 50",
  )
  .bind::<diesel::sql_types::Integer, _>(data.person_id)
  .bind::<diesel::sql_types::Bool, _>(is_me)
  .bind::<diesel::sql_types::Nullable<diesel::sql_types::Integer>, _>(me)
  .load(conn)
  .await?;
  Ok(Json(CreditedPostsResponse { posts }))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HideCreditedPost {
  pub post_id: i32,
  pub hidden: bool,
}

/// PUT /credited_posts/hide: hide (or show again) a post crediting you on your own profile.
pub async fn hide_credited_post(
  Json(data): Json<HideCreditedPost>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<HideCreditedPost>> {
  lemmy_api_utils::utils::check_local_user_banned_or_deleted(&local_user_view)?;
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  diesel::sql_query(if data.hidden {
    "INSERT INTO credited_post_hidden (person_id, post_id) VALUES ($1, $2) ON CONFLICT DO NOTHING"
  } else {
    "DELETE FROM credited_post_hidden WHERE person_id = $1 AND post_id = $2"
  })
  .bind::<diesel::sql_types::Integer, _>(local_user_view.person.id.0)
  .bind::<diesel::sql_types::Integer, _>(data.post_id)
  .execute(conn)
  .await?;
  Ok(Json(data))
}

// The tags that make a post Taboo, lower case without brackets (see the
// audioprn_post_filter_tags migration). Admins edit the list.
diesel::table! {
  taboo_tag (tag) {
    tag -> Text,
  }
}

const MAX_TABOO_TAGS: usize = 200;

#[derive(Debug, Serialize, Deserialize)]
pub struct TabooTags {
  pub tags: Vec<String>,
}

/// GET /taboo_tags: the Taboo list, A-Z.
pub async fn get_taboo_tags(context: Data<LemmyContext>) -> LemmyResult<Json<TabooTags>> {
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let tags = taboo_tag::table
    .order_by(taboo_tag::tag)
    .select(taboo_tag::tag)
    .load(conn)
    .await?;
  Ok(Json(TabooTags { tags }))
}

/// PUT /taboo_tags (admins): replaces the Taboo list and re-marks every post.
pub async fn set_taboo_tags(
  Json(data): Json<TabooTags>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<TabooTags>> {
  lemmy_api_utils::utils::is_admin(&local_user_view)?;
  // "[CNC], gore " -> ["cnc", "gore"]
  let mut tags: Vec<String> = data
    .tags
    .iter()
    .map(|t| t.trim().trim_start_matches('[').trim_end_matches(']').trim().to_lowercase())
    .filter(|t| !t.is_empty())
    .collect();
  tags.sort();
  tags.dedup();
  if tags.len() > MAX_TABOO_TAGS || tags.iter().any(|t| t.chars().count() > 40 || t.contains(['[', ']'])) {
    return Err(LemmyErrorType::Unknown("invalid_taboo_tags".into()).into());
  }
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let rows: Vec<_> = tags.iter().map(|t| taboo_tag::tag.eq(t)).collect();
  conn
    .transaction::<_, LemmyError, _>(async |conn| {
      diesel::delete(taboo_tag::table).execute(conn).await?;
      if !rows.is_empty() {
        insert_into(taboo_tag::table).values(&rows).execute(conn).await?;
      }
      diesel::sql_query(
        "UPDATE post_filter_tags t SET taboo = EXISTS
           (SELECT 1 FROM taboo_tag WHERE upper(tag) = ANY (t.tags))",
      )
      .execute(conn)
      .await?;
      Ok(())
    })
    .await?;
  Ok(Json(TabooTags { tags }))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn c(role: &str, name: &str) -> PostCredit {
    PostCredit {
      role: role.into(),
      name: name.into(),
    }
  }

  #[test]
  fn credits_are_trimmed_and_checked() {
    let ok = validate_credits(vec![c("writer", " u/anna "), c("voice_actor", ""), c("editor", "Bo")]);
    assert_eq!(ok.ok(), Some(vec![c("writer", "u/anna"), c("editor", "Bo")]));
    assert!(validate_credits(vec![c("director", "x")]).is_err());
    assert!(validate_credits(vec![c("writer", &"x".repeat(101))]).is_err());
    assert!(validate_credits(vec![c("writer", "x"); 31]).is_err());
  }

  #[test]
  fn test_is_multi_voice() {
    for multi in ["[FM4M] Hi", "[FF4A] x", "[M4F4M] x", "[F4M] [TFM4A] x", "[F4M] [Collab] x"] {
      assert!(is_multi_voice(multi), "{multi}");
    }
    for single in ["[F4M] x", "[TF4M] x", "[NB4A] x", "[A4M] x", "[F4M] [M4F] versions", "[F4M] collabo", "[F4M] [Self-Collab]"] {
      assert!(!is_multi_voice(single), "{single}");
    }
  }
}
