use lemmy_db_schema::source::local_user::LocalUser;
use lemmy_db_views_local_user::LocalUserView;
use lemmy_diesel_utils::sensitive::SensitiveString;
use lemmy_utils::{
  error::{LemmyErrorType, LemmyResult},
  settings::structs::Settings,
};
use translations::Lang;

pub mod account;
pub mod admin;
pub mod notifications;
mod send;

pub mod translations {
  rosetta_i18n::include_translations!();
}

fn inbox_link(settings: &Settings) -> String {
  format!("{}/inbox", settings.get_protocol_and_hostname())
}

/// audioprn: the site is English-only, so emails (and logged-in RSS feeds) are always English,
/// whatever language the user's browser sent at signup.
pub fn user_language(_local_user: &LocalUser) -> Lang {
  Lang::En
}

fn user_email(local_user_view: &LocalUserView) -> LemmyResult<SensitiveString> {
  local_user_view
    .local_user
    .email
    .clone()
    .ok_or(LemmyErrorType::EmailRequired.into())
}
