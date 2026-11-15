//! audioprn opt-outs: Reddit users who asked not to appear here. Anyone can send a takedown
//! request (/takedown); admins add names to the opt-out list (/admin/opt_outs). Adding a name
//! removes (as a moderator would, so it can be undone) the imported Reddit posts that mention it
//! (u/name, reddit.com/user/name, or a credit), and new posts, edits and credits can't mention
//! it. Masterlist entries are exempt. Creator applications show a matching name.

use activitypub_federation::config::Data;
use actix_web::web::Json;
use chrono::{DateTime, Utc};
use diesel::{
  QueryableByName,
  sql_query,
  sql_types::{Integer, Nullable, Text, Timestamptz},
};
use diesel_async::RunQueryDsl;
use lemmy_api_utils::{context::LemmyContext, utils::is_admin};
use lemmy_db_views_local_user::LocalUserView;
use lemmy_diesel_utils::connection::{DbPool, get_conn};
use lemmy_utils::error::{LemmyErrorType, LemmyResult};
use serde::{Deserialize, Serialize};

/// Open takedown requests kept at most (the form is open to everyone).
const MAX_OPEN_REQUESTS: i64 = 1000;

fn is_name_char(c: char) -> bool {
  c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

fn strip_u(name: &str) -> &str {
  let name = name.trim();
  let name = name.strip_prefix('/').unwrap_or(name);
  name
    .strip_prefix("u/")
    .or_else(|| name.strip_prefix("U/"))
    .unwrap_or(name)
}

/// A Reddit username without "u/": 3-20 letters, digits, _ or -.
fn reddit_name(name: &str) -> LemmyResult<String> {
  let name = strip_u(name);
  if (3..=20).contains(&name.len()) && name.chars().all(is_name_char) {
    Ok(name.to_string())
  } else {
    Err(LemmyErrorType::Unknown("invalid_reddit_name".into()).into())
  }
}

/// Whether the text mentions the Reddit user as u/name or user/name (reddit.com/user/name), not
/// as part of a longer name. Plain words don't count, so a name like "honey" is safe.
pub fn mentions(text: &str, name: &str) -> bool {
  let text = text.to_ascii_lowercase();
  let name = name.to_ascii_lowercase();
  ["u/", "user/"].iter().any(|prefix| {
    let needle = format!("{prefix}{name}");
    text.match_indices(&needle).any(|(i, _)| {
      !text[..i].chars().next_back().is_some_and(is_name_char)
        && !text[i + needle.len()..]
          .chars()
          .next()
          .is_some_and(is_name_char)
    })
  })
}

#[derive(QueryableByName)]
struct Name {
  #[diesel(sql_type = Text)]
  reddit_name: String,
}

pub(crate) async fn opt_out_names(pool: &mut DbPool<'_>) -> LemmyResult<Vec<String>> {
  let conn = &mut get_conn(pool).await?;
  Ok(
    sql_query("SELECT reddit_name FROM opt_out")
      .load::<Name>(conn)
      .await?
      .into_iter()
      .map(|n| n.reddit_name)
      .collect(),
  )
}

/// New posts, edits and credits can't mention an opted-out Reddit user.
pub async fn check_opt_out(
  pool: &mut DbPool<'_>,
  texts: &[Option<&str>],
  credits: &[&str],
) -> LemmyResult<()> {
  for name in opt_out_names(pool).await? {
    if texts.iter().flatten().any(|t| mentions(t, &name))
      || credits
        .iter()
        .any(|c| strip_u(c).eq_ignore_ascii_case(&name))
    {
      return Err(LemmyErrorType::Unknown(format!("opted_out:{name}")).into());
    }
  }
  Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TakedownRequestForm {
  pub reddit_name: String,
  #[serde(default)]
  pub contact: String,
  #[serde(default)]
  pub message: String,
}

/// POST /takedown_request (anyone; rate limited like signups).
pub async fn submit_takedown_request(
  Json(data): Json<TakedownRequestForm>,
  context: Data<LemmyContext>,
) -> LemmyResult<Json<TakedownRequestForm>> {
  let reddit_name = reddit_name(&data.reddit_name)?;
  let contact = data.contact.trim().to_string();
  let message = data.message.trim().to_string();
  if contact.chars().count() > 200 || message.chars().count() > 2000 {
    return Err(LemmyErrorType::Unknown("invalid_takedown_request".into()).into());
  }
  let mut pool = context.pool();
  let added = sql_query(
    "INSERT INTO takedown_request (reddit_name, contact, message) SELECT $1, $2, $3
     WHERE (SELECT count(*) FROM takedown_request) < $4",
  )
  .bind::<Text, _>(&reddit_name)
  .bind::<Text, _>(&contact)
  .bind::<Text, _>(&message)
  .bind::<diesel::sql_types::BigInt, _>(MAX_OPEN_REQUESTS)
  .execute(&mut get_conn(&mut pool).await?)
  .await?;
  if added == 0 {
    return Err(LemmyErrorType::Unknown("takedown_requests_full".into()).into());
  }
  Ok(Json(TakedownRequestForm {
    reddit_name,
    contact,
    message,
  }))
}

#[derive(Debug, Serialize, QueryableByName)]
pub struct TakedownRequest {
  #[diesel(sql_type = Integer)]
  pub id: i32,
  #[diesel(sql_type = Text)]
  pub reddit_name: String,
  #[diesel(sql_type = Text)]
  pub contact: String,
  #[diesel(sql_type = Text)]
  pub message: String,
  #[diesel(sql_type = Timestamptz)]
  pub published_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, QueryableByName)]
pub struct OptOut {
  #[diesel(sql_type = Integer)]
  pub id: i32,
  #[diesel(sql_type = Text)]
  pub reddit_name: String,
  #[diesel(sql_type = Integer)]
  pub posts_removed: i32,
  #[diesel(sql_type = Timestamptz)]
  pub published_at: DateTime<Utc>,
  #[diesel(sql_type = Nullable<Text>)]
  pub admin_name: Option<String>,
}

const OPT_OUT_COLUMNS: &str = "id, reddit_name, posts_removed, published_at,
  (SELECT name FROM person WHERE person.id = admin_id) AS admin_name";

#[derive(Debug, Serialize)]
pub struct OptOutsResponse {
  pub requests: Vec<TakedownRequest>,
  pub opt_outs: Vec<OptOut>,
}

/// GET /opt_outs (admins): open takedown requests (oldest first) and the opt-out list (A-Z).
pub async fn list_opt_outs(
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<OptOutsResponse>> {
  is_admin(&local_user_view)?;
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let requests = sql_query(
    "SELECT id, reddit_name, contact, message, published_at FROM takedown_request
     ORDER BY published_at",
  )
  .load(conn)
  .await?;
  let opt_outs = sql_query(format!(
    "SELECT {OPT_OUT_COLUMNS} FROM opt_out ORDER BY lower(reddit_name)"
  ))
  .load(conn)
  .await?;
  Ok(Json(OptOutsResponse { requests, opt_outs }))
}

#[derive(Debug, Deserialize)]
pub struct AddOptOut {
  pub reddit_name: String,
}

/// POST /opt_out (admins): adds the name, removes the imported posts that mention it, and closes
/// its takedown requests. Adding a listed name again removes newly matching posts.
pub async fn add_opt_out(
  Json(data): Json<AddOptOut>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<OptOut>> {
  is_admin(&local_user_view)?;
  let name = reddit_name(&data.reddit_name)?;
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  sql_query(
    "INSERT INTO opt_out (reddit_name, admin_id) VALUES ($1, $2)
     ON CONFLICT ((lower(reddit_name))) DO NOTHING",
  )
  .bind::<Text, _>(&name)
  .bind::<Integer, _>(local_user_view.person.id.0)
  .execute(conn)
  .await?;
  // Same rule as `mentions`: u/name or user/name, not inside a longer name; or a credit.
  let opt_out = sql_query(format!(
    "WITH hit AS (
       UPDATE post p SET removed = true
       WHERE p.id IN (SELECT post_id FROM reddit_import) AND NOT p.removed
         AND (concat_ws(' ', p.name, p.body, p.url)
                ~* ('(^|[^a-z0-9_-])(u|user)/' || $1 || '($|[^a-z0-9_-])')
              OR EXISTS (SELECT 1 FROM post_credit pc WHERE pc.post_id = p.id
                AND lower(regexp_replace(trim(pc.name), '^/?u/', '', 'i')) = lower($1)))
       RETURNING p.id)
     UPDATE opt_out SET posts_removed = posts_removed + (SELECT count(*) FROM hit)
     WHERE lower(reddit_name) = lower($1)
     RETURNING {OPT_OUT_COLUMNS}"
  ))
  .bind::<Text, _>(&name)
  .get_result::<OptOut>(conn)
  .await?;
  sql_query("DELETE FROM takedown_request WHERE lower(reddit_name) = lower($1)")
    .bind::<Text, _>(&name)
    .execute(conn)
    .await?;
  Ok(Json(opt_out))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OptOutId {
  pub id: i32,
}

/// DELETE /opt_out (admins): takes the name off the list. Removed posts stay removed.
pub async fn delete_opt_out(
  Json(data): Json<OptOutId>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<OptOutId>> {
  is_admin(&local_user_view)?;
  let mut pool = context.pool();
  sql_query("DELETE FROM opt_out WHERE id = $1")
    .bind::<Integer, _>(data.id)
    .execute(&mut get_conn(&mut pool).await?)
    .await?;
  Ok(Json(data))
}

/// DELETE /takedown_request (admins): dismisses a request without opting the name out.
pub async fn dismiss_takedown_request(
  Json(data): Json<OptOutId>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<OptOutId>> {
  is_admin(&local_user_view)?;
  let mut pool = context.pool();
  sql_query("DELETE FROM takedown_request WHERE id = $1")
    .bind::<Integer, _>(data.id)
    .execute(&mut get_conn(&mut pool).await?)
    .await?;
  Ok(Json(data))
}

#[cfg(test)]
mod tests {
  use super::mentions;

  #[test]
  fn test_mentions() {
    assert!(mentions("[F4M] with u/Honey_Voice!", "honey_voice"));
    assert!(mentions("https://reddit.com/user/honey_voice/", "Honey_Voice"));
    assert!(mentions("/u/honey_voice", "honey_voice"));
    assert!(!mentions("sweet honey_voice", "honey_voice"));
    assert!(!mentions("u/honey_voice2", "honey_voice"));
    assert!(!mentions("menu/honey_voice", "honey_voice"));
  }
}
