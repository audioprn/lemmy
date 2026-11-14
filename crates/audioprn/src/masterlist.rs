//! audioprn masterlists. Each creator has their own categories (an icon, the tags that put an
//! audio in it, `*` meaning any text, and categories added along with it) and entries for audios
//! elsewhere, free or paywalled. The masterlist page lists these with the creator's posts and
//! works out categories from tags.

use crate::{clean_link, is_verified_creator};
use activitypub_federation::config::Data;
use actix_web::web::{Json, Query};
use chrono::{DateTime, NaiveDate, Utc};
use diesel::{
  OptionalExtension,
  QueryableByName,
  sql_query,
  sql_types::{Array, BigInt, Bool, Date, Integer, Nullable, Text, Timestamptz},
};
use diesel_async::{AsyncConnection, RunQueryDsl};
use lemmy_api_utils::{context::LemmyContext, utils::check_local_user_banned_or_deleted};
use lemmy_db_views_local_user::LocalUserView;
use lemmy_diesel_utils::connection::get_conn;
use lemmy_utils::error::{LemmyError, LemmyErrorType, LemmyResult};
use serde::{Deserialize, Serialize};

const MAX_CATEGORIES: usize = 200;
const MAX_ALIASES: usize = 400;
const MAX_ALSO: usize = 20;
const MAX_ENTRIES: i64 = 5000;
/// Tags, categories and links on one entry.
const MAX_LIST: usize = 50;
const MAX_NAME: usize = 60;
const MAX_TITLE: usize = 300;
const MAX_DESCRIPTION: usize = 5000;

#[derive(Debug, Serialize, Deserialize, QueryableByName)]
pub struct MasterlistCategory {
  #[diesel(sql_type = Text)]
  pub name: String,
  /// An emoji (or the name of one of the site's icon files).
  #[diesel(sql_type = Text)]
  pub icon: String,
  /// Tags that put an audio in this category; `*` matches any text.
  #[diesel(sql_type = Array<Text>)]
  pub aliases: Vec<String>,
  /// Categories added along with this one.
  #[diesel(sql_type = Array<Text>)]
  pub also: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, QueryableByName)]
pub struct MasterlistEntry {
  /// 0 for a new entry.
  #[serde(default)]
  #[diesel(sql_type = Integer)]
  pub id: i32,
  #[diesel(sql_type = Text)]
  pub title: String,
  #[diesel(sql_type = Text)]
  pub description: String,
  #[diesel(sql_type = Nullable<Date>)]
  pub date: Option<NaiveDate>,
  #[diesel(sql_type = Text)]
  pub duration: String,
  #[diesel(sql_type = Text)]
  pub writer: String,
  #[diesel(sql_type = Nullable<Text>)]
  pub writer_link: Option<String>,
  /// Collaborators, as written.
  #[diesel(sql_type = Text)]
  pub credits: String,
  #[diesel(sql_type = Array<Text>)]
  pub tags: Vec<String>,
  /// None: worked out from the tags; otherwise the categories the creator chose.
  #[diesel(sql_type = Nullable<Array<Text>>)]
  pub categories: Option<Vec<String>>,
  #[diesel(sql_type = Array<Text>)]
  pub links: Vec<String>,
  #[diesel(sql_type = Nullable<Text>)]
  pub script_link: Option<String>,
  #[diesel(sql_type = Bool)]
  pub paywalled: bool,
}

#[derive(Debug, Serialize)]
pub struct MasterlistResponse {
  pub categories: Vec<MasterlistCategory>,
  pub entries: Vec<MasterlistEntry>,
  /// The creator's posts, when asked for.
  pub posts: Vec<MasterlistPost>,
  /// The viewer's saved posts and favourite entries on this masterlist.
  pub saved_posts: Vec<i32>,
  pub favourite_entries: Vec<i32>,
}

/// A creator's post on their masterlist.
#[derive(Debug, Serialize, QueryableByName)]
pub struct MasterlistPost {
  #[diesel(sql_type = Integer)]
  pub id: i32,
  #[diesel(sql_type = Text)]
  pub name: String,
  #[diesel(sql_type = Timestamptz)]
  pub published_at: DateTime<Utc>,
  #[diesel(sql_type = Nullable<Text>)]
  pub url: Option<String>,
  /// The start of the text.
  #[diesel(sql_type = Text)]
  pub body: String,
  /// Its tags (in capitals, as the content filters read them).
  #[diesel(sql_type = Array<Text>)]
  pub tags: Vec<String>,
  /// "role:name", e.g. "writer:u/anna".
  #[diesel(sql_type = Array<Text>)]
  pub credits: Vec<String>,
  /// On the creator's own page rather than the Content page.
  #[diesel(sql_type = Bool)]
  pub creator_page: bool,
}

#[derive(Debug, Deserialize)]
pub struct GetMasterlist {
  pub person_id: i32,
  /// Include the creator's posts.
  #[serde(default)]
  pub posts: bool,
}

/// GET /masterlist?person_id=1[&posts=true]: a creator's categories and entries (and posts),
/// newest first, with the viewer's saved posts and favourite entries.
pub async fn get_masterlist(
  Query(data): Query<GetMasterlist>,
  context: Data<LemmyContext>,
  local_user_view: Option<LocalUserView>,
) -> LemmyResult<Json<MasterlistResponse>> {
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let categories = sql_query(
    "SELECT name, icon, aliases, also FROM masterlist_category WHERE person_id = $1
     ORDER BY position",
  )
  .bind::<Integer, _>(data.person_id)
  .load(conn)
  .await?;
  let entries = sql_query(
    "SELECT id, title, description, date, duration, writer, writer_link, credits, tags,
       categories, links, script_link, paywalled
     FROM masterlist_entry WHERE person_id = $1 ORDER BY date DESC NULLS LAST, id DESC",
  )
  .bind::<Integer, _>(data.person_id)
  .load(conn)
  .await?;
  let posts = if data.posts {
    sql_query(
      "SELECT p.id, p.name, p.published_at, p.url, left(coalesce(p.body, ''), 600) AS body,
         coalesce(t.tags, '{}') AS tags,
         coalesce((SELECT array_agg(pc.role || ':' || pc.name) FROM post_credit pc
           WHERE pc.post_id = p.id), '{}') AS credits,
         c.name = 'creators' AS creator_page
       FROM post p JOIN community c ON c.id = p.community_id
       LEFT JOIN post_filter_tags t ON t.post_id = p.id
       WHERE p.creator_id = $1 AND NOT p.deleted AND NOT p.removed
         AND p.scheduled_publish_time_at IS NULL
       ORDER BY p.published_at DESC",
    )
    .bind::<Integer, _>(data.person_id)
    .load(conn)
    .await?
  } else {
    vec![]
  };
  let (saved_posts, favourite_entries) = match &local_user_view {
    Some(viewer) => {
      let ids = |sql| {
        sql_query(sql)
          .bind::<Integer, _>(viewer.person.id.0)
          .bind::<Integer, _>(data.person_id)
      };
      let saved: Vec<Id> = ids(
        "SELECT pa.post_id AS id FROM post_actions pa JOIN post p ON p.id = pa.post_id
         WHERE pa.person_id = $1 AND pa.saved_at IS NOT NULL AND p.creator_id = $2",
      )
      .load(conn)
      .await?;
      let favourites: Vec<Id> = ids(
        "SELECT f.entry_id AS id FROM masterlist_favourite f
         JOIN masterlist_entry e ON e.id = f.entry_id WHERE f.person_id = $1 AND e.person_id = $2",
      )
      .load(conn)
      .await?;
      (
        saved.into_iter().map(|i| i.id).collect(),
        favourites.into_iter().map(|i| i.id).collect(),
      )
    }
    None => (vec![], vec![]),
  };
  Ok(Json(MasterlistResponse {
    categories,
    entries,
    posts,
    saved_posts,
    favourite_entries,
  }))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetMasterlistFavourite {
  pub entry_id: i32,
  pub favourite: bool,
}

/// PUT /masterlist/favourite: the logged-in user (un)favourites a masterlist entry.
pub async fn set_favourite(
  Json(data): Json<SetMasterlistFavourite>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<SetMasterlistFavourite>> {
  let mut pool = context.pool();
  sql_query(if data.favourite {
    "INSERT INTO masterlist_favourite (person_id, entry_id) VALUES ($1, $2) ON CONFLICT DO NOTHING"
  } else {
    "DELETE FROM masterlist_favourite WHERE person_id = $1 AND entry_id = $2"
  })
  .bind::<Integer, _>(local_user_view.person.id.0)
  .bind::<Integer, _>(data.entry_id)
  .execute(&mut get_conn(&mut pool).await?)
  .await?;
  Ok(Json(data))
}

/// Only verified creators (and admins) keep a masterlist.
async fn check_creator(context: &LemmyContext, local_user_view: &LocalUserView) -> LemmyResult<()> {
  check_local_user_banned_or_deleted(local_user_view)?;
  if local_user_view.local_user.admin
    || is_verified_creator(&mut context.pool(), local_user_view.person.id).await?
  {
    Ok(())
  } else {
    Err(LemmyErrorType::Unknown("only_verified_creators_can_post".into()).into())
  }
}

fn invalid() -> LemmyError {
  LemmyErrorType::Unknown("invalid_masterlist".into()).into()
}

/// Trimmed, without blanks, each at most `max_len` characters and at most `max` of them.
fn clean_list(list: Vec<String>, max: usize, max_len: usize) -> LemmyResult<Vec<String>> {
  let list: Vec<String> = list
    .into_iter()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .collect();
  if list.len() > max || list.iter().any(|s| s.chars().count() > max_len) {
    Err(invalid())
  } else {
    Ok(list)
  }
}

fn clean_link_opt(link: Option<String>) -> LemmyResult<Option<String>> {
  link
    .filter(|l| !l.trim().is_empty())
    .map(|l| clean_link(&l))
    .transpose()
}

#[derive(Debug, Deserialize)]
pub struct SetMasterlistCategories {
  pub categories: Vec<MasterlistCategory>,
}

/// PUT /masterlist/categories: replace the creator's categories (their order is kept).
pub async fn set_categories(
  Json(data): Json<SetMasterlistCategories>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<MasterlistResponse>> {
  check_creator(&context, &local_user_view).await?;
  if data.categories.len() > MAX_CATEGORIES {
    return Err(invalid());
  }
  let mut categories = Vec::new();
  for c in data.categories {
    let name = c.name.trim().to_string();
    let icon = c.icon.trim().to_string();
    if name.is_empty()
      || name.chars().count() > MAX_NAME
      || icon.chars().count() > MAX_NAME
      || categories
        .iter()
        .any(|o: &MasterlistCategory| o.name.eq_ignore_ascii_case(&name))
    {
      return Err(invalid());
    }
    categories.push(MasterlistCategory {
      name,
      icon,
      aliases: clean_list(c.aliases, MAX_ALIASES, MAX_NAME)?,
      also: clean_list(c.also, MAX_ALSO, MAX_NAME)?,
    });
  }
  let me = local_user_view.person.id.0;
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  conn
    .transaction::<_, LemmyError, _>(async |conn| {
      sql_query("DELETE FROM masterlist_category WHERE person_id = $1")
        .bind::<Integer, _>(me)
        .execute(conn)
        .await?;
      for (position, c) in categories.iter().enumerate() {
        sql_query(
          "INSERT INTO masterlist_category (person_id, position, name, icon, aliases, also)
           VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind::<Integer, _>(me)
        .bind::<Integer, _>(i32::try_from(position).unwrap_or_default())
        .bind::<Text, _>(&c.name)
        .bind::<Text, _>(&c.icon)
        .bind::<Array<Text>, _>(&c.aliases)
        .bind::<Array<Text>, _>(&c.also)
        .execute(conn)
        .await?;
      }
      Ok(())
    })
    .await?;
  get_masterlist(
    Query(GetMasterlist {
      person_id: me,
      posts: false,
    }),
    context,
    None,
  )
  .await
}

#[derive(QueryableByName)]
struct Id {
  #[diesel(sql_type = Integer)]
  id: i32,
}

/// PUT /masterlist/entry: add (id 0) or update one of the creator's entries.
pub async fn save_entry(
  Json(e): Json<MasterlistEntry>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<MasterlistEntry>> {
  check_creator(&context, &local_user_view).await?;
  let entry = MasterlistEntry {
    id: e.id,
    title: e.title.trim().to_string(),
    description: e.description.trim().to_string(),
    date: e.date,
    duration: e.duration.trim().to_string(),
    writer: e.writer.trim().to_string(),
    writer_link: clean_link_opt(e.writer_link)?,
    credits: e.credits.trim().to_string(),
    tags: clean_list(e.tags, MAX_LIST, MAX_NAME)?,
    categories: e
      .categories
      .map(|c| clean_list(c, MAX_LIST, MAX_NAME))
      .transpose()?,
    links: e
      .links
      .iter()
      .filter(|l| !l.trim().is_empty())
      .map(|l| clean_link(l))
      .collect::<LemmyResult<_>>()?,
    script_link: clean_link_opt(e.script_link)?,
    paywalled: e.paywalled,
  };
  if entry.title.is_empty()
    || entry.title.chars().count() > MAX_TITLE
    || entry.description.chars().count() > MAX_DESCRIPTION
    || entry.duration.chars().count() > MAX_NAME
    || entry.writer.chars().count() > MAX_NAME * 2
    || entry.credits.chars().count() > MAX_TITLE
    || entry.links.len() > MAX_LIST
  {
    return Err(invalid());
  }
  let me = local_user_view.person.id.0;
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  // Both queries take the same parameters ($2 = id, $15 = the most entries one creator can have).
  let query = if entry.id == 0 {
    sql_query(
      "INSERT INTO masterlist_entry (person_id, title, description, date, duration, writer,
         writer_link, credits, tags, categories, links, script_link, paywalled)
       SELECT $1, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14
       WHERE $2 = 0 AND (SELECT count(*) FROM masterlist_entry WHERE person_id = $1) < $15
       RETURNING id",
    )
  } else {
    sql_query(
      "UPDATE masterlist_entry SET title = $3, description = $4, date = $5, duration = $6,
         writer = $7, writer_link = $8, credits = $9, tags = $10, categories = $11, links = $12,
         script_link = $13, paywalled = $14, updated_at = now()
       WHERE id = $2 AND person_id = $1 AND $15 > 0 RETURNING id",
    )
  };
  let id = query
    .bind::<Integer, _>(me)
    .bind::<Integer, _>(entry.id)
    .bind::<Text, _>(&entry.title)
    .bind::<Text, _>(&entry.description)
    .bind::<Nullable<Date>, _>(entry.date)
    .bind::<Text, _>(&entry.duration)
    .bind::<Text, _>(&entry.writer)
    .bind::<Nullable<Text>, _>(&entry.writer_link)
    .bind::<Text, _>(&entry.credits)
    .bind::<Array<Text>, _>(&entry.tags)
    .bind::<Nullable<Array<Text>>, _>(&entry.categories)
    .bind::<Array<Text>, _>(&entry.links)
    .bind::<Nullable<Text>, _>(&entry.script_link)
    .bind::<Bool, _>(entry.paywalled)
    .bind::<BigInt, _>(MAX_ENTRIES)
    .get_result::<Id>(conn)
    .await
    .optional()?
    .ok_or(if entry.id == 0 {
      LemmyErrorType::Unknown("too_many_masterlist_entries".into())
    } else {
      LemmyErrorType::NotFound
    })?
    .id;
  Ok(Json(MasterlistEntry { id, ..entry }))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteMasterlistEntry {
  pub id: i32,
}

/// DELETE /masterlist/entry: delete one of the creator's entries.
pub async fn delete_entry(
  Json(data): Json<DeleteMasterlistEntry>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<DeleteMasterlistEntry>> {
  let mut pool = context.pool();
  sql_query("DELETE FROM masterlist_entry WHERE id = $1 AND person_id = $2")
    .bind::<Integer, _>(data.id)
    .bind::<Integer, _>(local_user_view.person.id.0)
    .execute(&mut get_conn(&mut pool).await?)
    .await?;
  Ok(Json(data))
}
