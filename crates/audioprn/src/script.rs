//! audioprn scripts on Script Offer posts: the script's text (a backup of scriptbin.works or
//! another site, so readers can stay here), an optional link to where else it's hosted, and the
//! writer's fill terms. The text is for logged-in readers only, at most 30 different scripts an
//! hour per account (the writer and admins aren't limited). The writer adds or updates it.
//! Each reader's copy carries an invisible watermark (their account id in zero-width
//! characters on every line), so admins can tell whose copy a leaked script came from.

use crate::clean_link;
use activitypub_federation::config::Data;
use actix_web::web::{Json, Query};
use chrono::{DateTime, Utc};
use diesel::{
  OptionalExtension,
  QueryableByName,
  sql_query,
  sql_types::{Array, BigInt, Bool, Integer, Nullable, Text, Timestamptz},
};
use diesel_async::RunQueryDsl;
use lemmy_api_utils::{
  context::LemmyContext,
  utils::{check_local_user_banned_or_deleted, is_admin},
};
use lemmy_db_views_local_user::LocalUserView;
use lemmy_diesel_utils::connection::get_conn;
use lemmy_utils::error::{LemmyErrorType, LemmyResult};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

const MAX_SCRIPT_LENGTH: usize = 200_000;
const READS_PER_HOUR: i64 = 30;
/// Ask first / Fine to paywall with credit given and copy sent / Temporary paywalling only /
/// No paywalled fills.
const FILL_TERMS: [&str; 4] = ["ask_first", "paywall_ok", "temporary_paywall", "no_paywall"];
/// A watermark: word joiner, the account id in binary (zero-width space = 0, zero-width
/// non-joiner = 1), word joiner.
static WATERMARK: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new("\u{2060}([\u{200B}\u{200C}]+)\u{2060}").expect("compile regex")
});

/// The text with the reader's watermark after the first space between words of every line.
fn watermark(body: &str, person_id: i32) -> String {
  let bits: String = format!("{person_id:b}")
    .chars()
    .map(|b| if b == '0' { '\u{200B}' } else { '\u{200C}' })
    .collect();
  let mark = format!(" \u{2060}{bits}\u{2060}");
  body
    .split('\n')
    .map(|line| {
      // Not inside the indentation, which markdown reads (nested lists).
      let (indent, text) = line.split_at(line.len() - line.trim_start().len());
      format!("{indent}{}", text.replacen(' ', &mark, 1))
    })
    .collect::<Vec<_>>()
    .join("\n")
}

#[derive(Debug, Serialize, QueryableByName)]
pub struct PostScript {
  #[diesel(sql_type = Integer)]
  pub post_id: i32,
  #[diesel(sql_type = Nullable<Text>)]
  pub link: Option<String>,
  #[diesel(sql_type = Nullable<Text>)]
  pub fill_terms: Option<String>,
  #[diesel(sql_type = Timestamptz)]
  pub updated_at: DateTime<Utc>,
  #[diesel(sql_type = BigInt)]
  pub words: i64,
  #[diesel(sql_type = Integer)]
  pub creator_id: i32,
  #[diesel(sql_type = Bool)]
  pub visible: bool,
  /// The text, only when asked for by a logged-in reader.
  #[diesel(sql_type = Nullable<Text>)]
  pub body: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PostScriptResponse {
  pub script: Option<PostScript>,
}

#[derive(Debug, Deserialize)]
pub struct GetPostScript {
  pub post_id: i32,
  /// Also return the text (logged in only; counts toward the reading limit).
  #[serde(default)]
  pub text: bool,
}

/// GET /post_script?post_id=1[&text=true]: a post's script details; the text only when asked for
/// by a logged-in reader within the reading limit.
pub async fn get_post_script(
  Query(data): Query<GetPostScript>,
  context: Data<LemmyContext>,
  local_user_view: Option<LocalUserView>,
) -> LemmyResult<Json<PostScriptResponse>> {
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let script: Option<PostScript> = sql_query(
    "SELECT s.post_id, s.link, s.fill_terms, s.updated_at,
       CASE WHEN trim(s.body) = '' THEN 0
         ELSE array_length(regexp_split_to_array(trim(s.body), '\\s+'), 1) END::bigint AS words,
       p.creator_id, NOT p.deleted AND NOT p.removed AS visible,
       CASE WHEN $2 THEN s.body END AS body
     FROM post_script s JOIN post p ON p.id = s.post_id WHERE s.post_id = $1",
  )
  .bind::<Integer, _>(data.post_id)
  .bind::<Bool, _>(data.text)
  .get_result(conn)
  .await
  .optional()?;
  let Some(mut script) = script else {
    return Ok(Json(PostScriptResponse { script: None }));
  };
  crate::blocks::check_not_blocked_by(
    &mut context.pool(),
    local_user_view.as_ref(),
    lemmy_db_schema_file::PersonId(script.creator_id),
  )
  .await?;
  // A deleted or removed post's script details are only for its writer and admins.
  if !script.visible
    && !local_user_view
      .as_ref()
      .is_some_and(|u| u.person.id.0 == script.creator_id || u.local_user.admin)
  {
    return Ok(Json(PostScriptResponse { script: None }));
  }
  if data.text {
    let Some(user) = local_user_view else {
      return Err(LemmyErrorType::Unknown("log_in_to_read_scripts".into()).into());
    };
    check_local_user_banned_or_deleted(&user)?;
    let me = user.person.id.0;
    if !script.visible {
      return Err(LemmyErrorType::NotFound.into());
    }
    if me != script.creator_id && !user.local_user.admin {
      let read_lately: i64 = sql_query(
        "SELECT count(*) AS n FROM post_script_read
         WHERE person_id = $1 AND post_id <> $2 AND read_at > now() - interval '1 hour'",
      )
      .bind::<Integer, _>(me)
      .bind::<Integer, _>(data.post_id)
      .get_result::<Count>(conn)
      .await?
      .n;
      if read_lately >= READS_PER_HOUR {
        return Err(LemmyErrorType::Unknown("script_read_limit".into()).into());
      }
      sql_query(
        "INSERT INTO post_script_read (person_id, post_id) VALUES ($1, $2)
         ON CONFLICT (person_id, post_id) DO UPDATE SET read_at = now()",
      )
      .bind::<Integer, _>(me)
      .bind::<Integer, _>(data.post_id)
      .execute(conn)
      .await?;
      script.body = script.body.map(|b| watermark(&b, me));
    }
  }
  Ok(Json(PostScriptResponse {
    script: Some(script),
  }))
}

#[derive(Debug, Deserialize)]
pub struct CheckWatermark {
  pub text: String,
}

#[derive(Debug, Serialize)]
pub struct CheckWatermarkResponse {
  /// Usernames whose watermark is in the text.
  pub readers: Vec<String>,
}

/// POST /script_watermark (admins): whose copy a pasted (leaked) script came from.
pub async fn check_watermark(
  Json(data): Json<CheckWatermark>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<CheckWatermarkResponse>> {
  is_admin(&local_user_view)?;
  let ids: Vec<i32> = WATERMARK
    .captures_iter(&data.text)
    .filter_map(|c| {
      let bits: String = c[1]
        .chars()
        .map(|z| if z == '\u{200B}' { '0' } else { '1' })
        .collect();
      i32::from_str_radix(&bits, 2).ok()
    })
    .collect();
  #[derive(QueryableByName)]
  struct Name {
    #[diesel(sql_type = Text)]
    name: String,
  }
  let mut pool = context.pool();
  let readers = sql_query("SELECT name FROM person WHERE id = ANY($1) ORDER BY name")
    .bind::<Array<Integer>, _>(ids)
    .load::<Name>(&mut get_conn(&mut pool).await?)
    .await?
    .into_iter()
    .map(|n| n.name)
    .collect();
  Ok(Json(CheckWatermarkResponse { readers }))
}

#[derive(QueryableByName)]
struct Count {
  #[diesel(sql_type = BigInt)]
  n: i64,
}

#[derive(Debug, Deserialize)]
pub struct SetPostScript {
  pub post_id: i32,
  #[serde(default)]
  pub body: String,
  pub link: Option<String>,
  pub fill_terms: Option<String>,
}

/// PUT /post_script: the post's writer (or an admin) adds, updates or (empty) removes the script.
/// Only on posts with the Script Offer flair.
pub async fn set_post_script(
  Json(data): Json<SetPostScript>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<PostScriptResponse>> {
  check_local_user_banned_or_deleted(&local_user_view)?;
  let body = data.body.trim().to_string();
  let link = data
    .link
    .filter(|l| !l.trim().is_empty())
    .map(|l| clean_link(&l))
    .transpose()?;
  let fill_terms = data.fill_terms.filter(|t| !t.is_empty());
  if body.chars().count() > MAX_SCRIPT_LENGTH
    || fill_terms.as_deref().is_some_and(|t| !FILL_TERMS.contains(&t))
  {
    return Err(LemmyErrorType::Unknown("invalid_script".into()).into());
  }
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  #[derive(QueryableByName)]
  struct PostInfo {
    #[diesel(sql_type = Integer)]
    creator_id: i32,
    #[diesel(sql_type = Bool)]
    script_offer: bool,
  }
  let post: PostInfo = sql_query(
    "SELECT p.creator_id, EXISTS (SELECT 1 FROM post_community_tag pct
       JOIN community_tag ct ON ct.id = pct.community_tag_id
       WHERE pct.post_id = p.id AND ct.name = 'script_offer' AND NOT ct.deleted) AS script_offer
     FROM post p WHERE p.id = $1",
  )
  .bind::<Integer, _>(data.post_id)
  .get_result(conn)
  .await
  .optional()?
  .ok_or(LemmyErrorType::NotFound)?;
  if post.creator_id != local_user_view.person.id.0 && !local_user_view.local_user.admin {
    return Err(LemmyErrorType::NoPostEditAllowed.into());
  }
  if !post.script_offer {
    return Err(LemmyErrorType::Unknown("scripts_only_on_script_offer_posts".into()).into());
  }
  if body.is_empty() && link.is_none() {
    sql_query("DELETE FROM post_script WHERE post_id = $1")
      .bind::<Integer, _>(data.post_id)
      .execute(conn)
      .await?;
    return Ok(Json(PostScriptResponse { script: None }));
  }
  sql_query(
    "INSERT INTO post_script (post_id, body, link, fill_terms) VALUES ($1, $2, $3, $4)
     ON CONFLICT (post_id) DO UPDATE SET body = $2, link = $3, fill_terms = $4, updated_at = now()",
  )
  .bind::<Integer, _>(data.post_id)
  .bind::<Text, _>(&body)
  .bind::<Nullable<Text>, _>(&link)
  .bind::<Nullable<Text>, _>(&fill_terms)
  .execute(conn)
  .await?;
  // The writer's own copy back, with the text.
  get_post_script(
    Query(GetPostScript {
      post_id: data.post_id,
      text: true,
    }),
    context,
    Some(local_user_view),
  )
  .await
}

#[derive(Debug, Serialize, QueryableByName)]
pub struct ScriptbinPost {
  #[diesel(sql_type = Integer)]
  pub post_id: i32,
  /// The script's address on scriptbin (scriptbin.works/u/name/<slug>).
  #[diesel(sql_type = Text)]
  pub slug: String,
  /// The post already holds a script text.
  #[diesel(sql_type = Bool)]
  pub has_script: bool,
  #[diesel(sql_type = Nullable<Text>)]
  pub link: Option<String>,
  #[diesel(sql_type = Nullable<Text>)]
  pub fill_terms: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ScriptbinPostsResponse {
  pub posts: Vec<ScriptbinPost>,
}

/// GET /scriptbin_posts: the logged-in creator's Script Offer posts that link a scriptbin script
/// (their link, or else the first one in their text: other scripts may be mentioned too), so the
/// scriptbin import can add each script's text to its post.
pub async fn list_scriptbin_posts(
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<ScriptbinPostsResponse>> {
  let mut pool = context.pool();
  let posts = sql_query(
    r"SELECT * FROM (SELECT p.id AS post_id,
         coalesce(substring(p.url FROM $2), substring(p.body FROM $2)) AS slug,
         coalesce(s.body, '') <> '' AS has_script, s.link, s.fill_terms
       FROM post p LEFT JOIN post_script s ON s.post_id = p.id
       WHERE p.creator_id = $1 AND NOT p.removed
         AND EXISTS (SELECT 1 FROM post_community_tag pct
           JOIN community_tag ct ON ct.id = pct.community_tag_id
           WHERE pct.post_id = p.id AND ct.name = 'script_offer' AND NOT ct.deleted)) AS p
     WHERE slug IS NOT NULL",
  )
  .bind::<Integer, _>(local_user_view.person.id.0)
  .bind::<Text, _>(r"(?i)scriptbin\.works/u/[^/\s]+/([A-Za-z0-9_-]+)")
  .load(&mut get_conn(&mut pool).await?)
  .await?;
  Ok(Json(ScriptbinPostsResponse { posts }))
}
