//! audioprn fork additions: verified creators.
//!
//! Creators apply (on the creator signup form, or later from their account) with exactly three
//! links to their audios or scripts, plus any number of optional social media links. An admin
//! approves or denies the application. Only approved ("verified") creators and admins can post.
//!
//! Kept in its own crate, with its own table definition, so upstream Lemmy merges stay simple.

pub mod awards;
pub mod blocks;
pub mod creator_page;
pub mod creators;
pub mod drafts;
pub mod masterlist;
pub mod feed;
pub mod news;
pub mod optout;
pub mod poll;
pub mod post;
pub mod reaction;
pub mod reddit;
pub mod script;
pub mod search;

use activitypub_federation::config::Data;
use actix_web::web::{Json, Query};
use chrono::{DateTime, Utc};
use diesel::{
  ExpressionMethods,
  OptionalExtension,
  QueryDsl,
  SelectableHelper,
  dsl::insert_into,
};
use diesel_async::RunQueryDsl;
use lemmy_api_utils::{
  context::LemmyContext,
  utils::{is_admin, is_mod_or_admin},
};
use lemmy_db_schema_file::{
  PersonId,
  newtypes::CommunityId,
  schema::{local_user, person},
};
use lemmy_db_views_local_user::LocalUserView;
use lemmy_diesel_utils::connection::{DbPool, get_conn};
use lemmy_utils::error::{LemmyErrorExt, LemmyErrorType, LemmyResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;

pub const CONTENT_LINKS_REQUIRED: usize = 3;
/// Social links are add-as-many-as-needed in the form; this cap only stops abuse.
pub const MAX_SOCIAL_LINKS: usize = 20;
const MAX_LINK_LENGTH: usize = 512;

diesel::table! {
  creator_application (id) {
    id -> Int4,
    person_id -> Int4,
    content_links -> Array<Text>,
    social_links -> Array<Text>,
    approved -> Nullable<Bool>,
    admin_id -> Nullable<Int4>,
    deny_reason -> Nullable<Text>,
    published_at -> Timestamptz,
    updated_at -> Nullable<Timestamptz>,
    hide_follower_count -> Bool,
    footer_blurb -> Text,
    footer_links -> Array<Text>,
    support_text -> Text,
  }
}

// Note: no joins with Lemmy's tables. Diesel's join macros can't link a table defined here
// with tables from another crate (orphan rule), so related rows are loaded in separate queries.

#[derive(Clone, Debug, Serialize, Deserialize, diesel::Queryable, diesel::Selectable)]
#[diesel(table_name = creator_application)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct CreatorApplication {
  pub id: i32,
  pub person_id: PersonId,
  pub content_links: Vec<String>,
  pub social_links: Vec<String>,
  /// None = pending, Some(true) = approved, Some(false) = denied.
  pub approved: Option<bool>,
  pub admin_id: Option<PersonId>,
  pub deny_reason: Option<String>,
  pub published_at: DateTime<Utc>,
  pub updated_at: Option<DateTime<Utc>>,
}

/// An application with the applicant's name, for the admin review page.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreatorApplicationView {
  pub creator_application: CreatorApplication,
  pub person_name: String,
  pub person_display_name: Option<String>,
  pub email_verified: bool,
  /// An opted-out Reddit name matching the username or mentioned in a link.
  pub opted_out: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Validation and database helpers (also used by the signup handler)
// ---------------------------------------------------------------------------------------------

fn clean_link(link: &str) -> LemmyResult<String> {
  let link = link.trim();
  if link.len() > MAX_LINK_LENGTH {
    return Err(LemmyErrorType::UrlLengthOverflow.into());
  }
  let url = Url::parse(link).with_lemmy_type(LemmyErrorType::InvalidUrl)?;
  if url.scheme() != "https" && url.scheme() != "http" {
    return Err(LemmyErrorType::InvalidUrlScheme.into());
  }
  Ok(url.to_string())
}

/// Checks the creator form's links. Returns cleaned (content_links, social_links).
///
/// Content links: exactly 3, all different. Social links: blank ones are dropped, at most MAX_SOCIAL_LINKS.
pub fn validate_creator_links(
  content_links: &[String],
  social_links: &[String],
) -> LemmyResult<(Vec<String>, Vec<String>)> {
  let content: Vec<String> = content_links
    .iter()
    .filter(|l| !l.trim().is_empty())
    .map(|l| clean_link(l))
    .collect::<LemmyResult<_>>()?;
  if content.len() != CONTENT_LINKS_REQUIRED {
    return Err(LemmyErrorType::Unknown("creator_three_content_links_required".into()).into());
  }
  let mut unique = content.clone();
  unique.sort();
  unique.dedup();
  if unique.len() != content.len() {
    return Err(LemmyErrorType::Unknown("creator_content_links_must_be_different".into()).into());
  }

  let social: Vec<String> = social_links
    .iter()
    .filter(|l| !l.trim().is_empty())
    .map(|l| clean_link(l))
    .collect::<LemmyResult<_>>()?;
  if social.len() > MAX_SOCIAL_LINKS {
    return Err(LemmyErrorType::Unknown("creator_too_many_social_links".into()).into());
  }

  Ok((content, social))
}

/// Creates a pending application. If the person was denied before, resubmits it as pending.
/// Fails if they already have a pending or approved application.
pub async fn submit_application(
  pool: &mut DbPool<'_>,
  person_id: PersonId,
  content_links: Vec<String>,
  social_links: Vec<String>,
) -> LemmyResult<CreatorApplication> {
  let conn = &mut get_conn(pool).await?;
  let existing: Option<CreatorApplication> = creator_application::table
    .filter(creator_application::person_id.eq(person_id.0))
    .select(CreatorApplication::as_select())
    .first(conn)
    .await
    .optional()?;

  match existing {
    Some(app) if app.approved != Some(false) => Err(LemmyErrorType::AlreadyExists.into()),
    Some(app) => diesel::update(creator_application::table.find(app.id))
      .set((
        creator_application::content_links.eq(content_links),
        creator_application::social_links.eq(social_links),
        creator_application::approved.eq(None::<bool>),
        creator_application::admin_id.eq(None::<i32>),
        creator_application::deny_reason.eq(None::<String>),
        creator_application::published_at.eq(Utc::now()),
        creator_application::updated_at.eq(None::<DateTime<Utc>>),
      ))
      .returning(CreatorApplication::as_returning())
      .get_result(conn)
      .await
      .with_lemmy_type(LemmyErrorType::CouldntUpdate),
    None => insert_into(creator_application::table)
      .values((
        creator_application::person_id.eq(person_id.0),
        creator_application::content_links.eq(content_links),
        creator_application::social_links.eq(social_links),
      ))
      .returning(CreatorApplication::as_returning())
      .get_result(conn)
      .await
      .with_lemmy_type(LemmyErrorType::CouldntCreate),
  }
}

pub async fn is_verified_creator(pool: &mut DbPool<'_>, person_id: PersonId) -> LemmyResult<bool> {
  let conn = &mut get_conn(pool).await?;
  let approved: Option<Option<bool>> = creator_application::table
    .filter(creator_application::person_id.eq(person_id.0))
    .select(creator_application::approved)
    .first(conn)
    .await
    .optional()?;
  Ok(approved == Some(Some(true)))
}

/// Admins, moderators of the community and verified creators may post.
pub async fn check_can_post(
  pool: &mut DbPool<'_>,
  local_user_view: &LocalUserView,
  community_id: CommunityId,
) -> LemmyResult<()> {
  if is_mod_or_admin(pool, local_user_view, community_id).await.is_ok()
    || is_verified_creator(pool, local_user_view.person.id).await?
  {
    Ok(())
  } else {
    Err(LemmyErrorType::Unknown("only_verified_creators_can_post".into()).into())
  }
}

// ---------------------------------------------------------------------------------------------
// API handlers, mounted under /api/v4/audioprn
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SubmitCreatorApplication {
  pub content_links: Vec<String>,
  #[serde(default)]
  pub social_links: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CreatorApplicationResponse {
  pub creator_application: Option<CreatorApplication>,
}

/// POST /creator_application: a logged-in user applies (or re-applies after a denial).
pub async fn submit_creator_application(
  Json(data): Json<SubmitCreatorApplication>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<CreatorApplicationResponse>> {
  lemmy_api_utils::utils::check_local_user_banned_or_deleted(&local_user_view)?;
  let (content, social) = validate_creator_links(&data.content_links, &data.social_links)?;
  let app = submit_application(
    &mut context.pool(),
    local_user_view.person.id,
    content,
    social,
  )
  .await?;
  Ok(Json(CreatorApplicationResponse {
    creator_application: Some(app),
  }))
}

/// GET /creator_application/mine: the logged-in user's own application, if any.
pub async fn get_my_creator_application(
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<CreatorApplicationResponse>> {
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let app = creator_application::table
    .filter(creator_application::person_id.eq(local_user_view.person.id.0))
    .select(CreatorApplication::as_select())
    .first(conn)
    .await
    .optional()?;
  Ok(Json(CreatorApplicationResponse {
    creator_application: app,
  }))
}

#[derive(Debug, Deserialize)]
pub struct ListCreatorApplications {
  /// Only pending applications. Defaults to true.
  pub pending_only: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ListCreatorApplicationsResponse {
  pub creator_applications: Vec<CreatorApplicationView>,
}

/// GET /creator_application/list (admins only): pending first, oldest first.
pub async fn list_creator_applications(
  Query(data): Query<ListCreatorApplications>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<ListCreatorApplicationsResponse>> {
  is_admin(&local_user_view)?;
  let opted_out_names = optout::opt_out_names(&mut context.pool()).await?;
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;

  let mut query = creator_application::table
    .select(CreatorApplication::as_select())
    .order_by(creator_application::published_at.asc())
    .limit(200)
    .into_boxed();
  if data.pending_only.unwrap_or(true) {
    query = query.filter(creator_application::approved.is_null());
  }
  let apps: Vec<CreatorApplication> = query.load(conn).await?;

  // Applicant names and email status, loaded separately (see note at the table definition).
  let ids: Vec<i32> = apps.iter().map(|a| a.person_id.0).collect();
  let names: HashMap<i32, (String, Option<String>)> = person::table
    .filter(person::id.eq_any(&ids))
    .select((person::id, person::name, person::display_name))
    .load::<(i32, String, Option<String>)>(conn)
    .await?
    .into_iter()
    .map(|(id, name, display_name)| (id, (name, display_name)))
    .collect();
  let verified: HashMap<i32, bool> = local_user::table
    .filter(local_user::person_id.eq_any(&ids))
    .select((local_user::person_id, local_user::email_verified))
    .load::<(i32, bool)>(conn)
    .await?
    .into_iter()
    .collect();

  let creator_applications = apps
    .into_iter()
    .map(|creator_application| {
      let id = creator_application.person_id.0;
      let (person_name, person_display_name) = names.get(&id).cloned().unwrap_or_default();
      let opted_out = opted_out_names
        .iter()
        .find(|n| {
          person_name.eq_ignore_ascii_case(n)
            || creator_application
              .content_links
              .iter()
              .chain(&creator_application.social_links)
              .any(|l| optout::mentions(l, n))
        })
        .cloned();
      CreatorApplicationView {
        creator_application,
        person_name,
        person_display_name,
        email_verified: verified.get(&id).copied().unwrap_or(false),
        opted_out,
      }
    })
    .collect();

  Ok(Json(ListCreatorApplicationsResponse {
    creator_applications,
  }))
}

#[derive(Debug, Deserialize)]
pub struct ApproveCreatorApplication {
  pub id: i32,
  pub approve: bool,
  pub deny_reason: Option<String>,
}

/// PUT /creator_application/approve (admins only): approve or deny. Approving makes the person a
/// verified creator; denying with approve=false also revokes an earlier approval.
pub async fn approve_creator_application(
  Json(data): Json<ApproveCreatorApplication>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<CreatorApplicationResponse>> {
  is_admin(&local_user_view)?;
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let deny_reason = if data.approve {
    None
  } else {
    data.deny_reason.filter(|r| !r.trim().is_empty())
  };

  let app = diesel::update(creator_application::table.find(data.id))
    .set((
      creator_application::approved.eq(Some(data.approve)),
      creator_application::admin_id.eq(Some(local_user_view.person.id.0)),
      creator_application::deny_reason.eq(deny_reason),
      creator_application::updated_at.eq(Some(Utc::now())),
    ))
    .returning(CreatorApplication::as_returning())
    .get_result(conn)
    .await
    .with_lemmy_type(LemmyErrorType::NotFound)?;

  // Only verified creators can be moderators.
  if !data.approve {
    diesel::sql_query("UPDATE community_actions SET became_moderator_at = NULL WHERE person_id = $1")
      .bind::<diesel::sql_types::Integer, _>(app.person_id.0)
      .execute(conn)
      .await?;
  }

  Ok(Json(CreatorApplicationResponse {
    creator_application: Some(app),
  }))
}

/// POST /account/banner: no images on the site except avatars, and banners for verified creators
/// and admins. (Other uploads are off with the site's `image_upload_disabled` setting.)
pub async fn upload_user_banner(
  req: actix_web::HttpRequest,
  body: actix_web::web::Payload,
  local_user_view: LocalUserView,
  context: actix_web::web::Data<LemmyContext>,
) -> LemmyResult<Json<lemmy_db_views_local_image::api::UploadImageResponse>> {
  if !local_user_view.local_user.admin
    && !is_verified_creator(&mut context.pool(), local_user_view.person.id).await?
  {
    return Err(LemmyErrorType::Unknown("only_creators_can_have_a_banner".into()).into());
  }
  lemmy_routes::images::upload::upload_user_banner(req, body, local_user_view, context).await
}

#[derive(Debug, Serialize)]
pub struct VerifiedCreatorsResponse {
  pub person_ids: Vec<PersonId>,
}

/// GET /verified_creators: ids of all verified creators, for showing badges.
pub async fn list_verified_creators(
  context: Data<LemmyContext>,
) -> LemmyResult<Json<VerifiedCreatorsResponse>> {
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let ids: Vec<i32> = creator_application::table
    .filter(creator_application::approved.eq(true))
    .select(creator_application::person_id)
    .load(conn)
    .await?;
  Ok(Json(VerifiedCreatorsResponse {
    person_ids: ids.into_iter().map(PersonId).collect(),
  }))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| (*x).to_string()).collect()
  }

  #[test]
  fn requires_exactly_three_different_content_links() {
    let three = s(&["https://a.example/1", "https://b.example/2", "https://c.example/3"]);
    assert!(validate_creator_links(&three, &[]).is_ok());
    assert!(validate_creator_links(&three[..2], &[]).is_err());
    let dup = s(&["https://a.example/1", "https://a.example/1", "https://c.example/3"]);
    assert!(validate_creator_links(&dup, &[]).is_err());
    let bad = s(&["https://a.example/1", "ftp://b.example/2", "https://c.example/3"]);
    assert!(validate_creator_links(&bad, &[]).is_err());
  }

  #[test]
  fn social_links_are_optional_and_capped() {
    let three = s(&["https://a.example/1", "https://b.example/2", "https://c.example/3"]);
    let (_, social) = validate_creator_links(&three, &s(&["", "  "])).unwrap_or_default();
    assert!(social.is_empty());
    let many: Vec<String> = (0..MAX_SOCIAL_LINKS)
      .map(|i| format!("https://social{i}.example"))
      .collect();
    assert!(validate_creator_links(&three, &many).is_ok());
    let too_many: Vec<String> = (0..=MAX_SOCIAL_LINKS)
      .map(|i| format!("https://social{i}.example"))
      .collect();
    assert!(validate_creator_links(&three, &too_many).is_err());
  }
}
