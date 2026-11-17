use actix_web::{guard, web::*};
use lemmy_api::{
  comment::{
    distinguish::distinguish_comment,
    like::like_comment,
    list_comment_likes::list_comment_likes,
    lock::lock_comment,
    save::save_comment,
    warning::create_comment_warning,
  },
  community::{
    add_mod::add_mod_to_community,
    ban::ban_from_community,
    block::user_block_community,
    follow::follow_community,
    multi_community_follow::follow_multi_community,
    pending_follows::{approve::post_pending_follows_approve, list::get_pending_follows_list},
    random::get_random_community,
    tag::{create_community_tag, delete_community_tag, edit_community_tag},
    transfer::transfer_community,
    update_notifications::edit_community_notifications,
  },
  federation::{
    list_comments::{list_comments, list_comments_slim},
    list_person_content::list_person_content,
    list_persons::list_persons,
    list_posts::list_posts,
    read_community::get_community,
    read_multi_community::read_multi_community,
    read_person::read_person,
    resolve_object::resolve_object,
    search::search,
    user_settings_backup::{export_user_settings, import_user_settings},
  },
  local_user::{
    add_admin::add_admin,
    ban_person::ban_from_site,
    block::user_block_person,
    change_password::change_password,
    change_password_after_reset::change_password_after_reset,
    export_data::export_user_data,
    generate_totp_secret::generate_totp_secret,
    get_captcha::get_captcha,
    list_hidden::list_person_hidden,
    list_liked::list_person_liked,
    list_logins::list_logins,
    list_media::list_media,
    list_read::list_person_read,
    list_saved::list_person_saved,
    login::login,
    logout::logout,
    mark_donation_dialog_shown::mark_donation_dialog_shown,
    note_person::user_note_person,
    notifications::{
      list::list_notifications,
      mark_all_read::mark_all_notifications_read,
      mark_notification_read::mark_notification_as_read,
    },
    resend_verification_email::resend_verification_email,
    reset_password::reset_password,
    save_settings::save_user_settings,
    unread_counts::get_unread_counts,
    update_totp::edit_totp,
    user_block_instance::{user_block_instance_communities, user_block_instance_persons},
    validate_auth::validate_auth,
    verify_email::verify_email,
  },
  post::{
    feature::feature_post,
    get_link_metadata::get_link_metadata,
    hide::hide_post,
    like::like_post,
    list_post_likes::list_post_likes,
    lock::lock_post,
    mark_many_read::mark_posts_as_read,
    mark_read::mark_post_as_read,
    mod_update::mod_edit_post,
    save::save_post,
    update_notifications::edit_post_notifications,
    warning::create_post_warning,
  },
  reports::{
    comment_report::{create::create_comment_report, resolve::resolve_comment_report},
    community_report::{create::create_community_report, resolve::resolve_community_report},
    post_report::{create::create_post_report, resolve::resolve_post_report},
    private_message_report::{create::create_pm_report, resolve::resolve_pm_report},
    report_combined::list::list_reports,
  },
  site::{
    admin_allow_instance::admin_allow_instance,
    admin_block_instance::admin_block_instance,
    admin_list_users::admin_list_users,
    federated_instances::get_federated_instances,
    list_all_media::list_all_media,
    mod_log::get_mod_log,
    purge::{
      comment::purge_comment,
      community::purge_community,
      person::purge_person,
      post::purge_post,
    },
    registration_applications::{
      approve::approve_registration_application,
      get::get_registration_application,
      list::list_registration_applications,
    },
  },
};
use lemmy_api_crud::{
  comment::{
    create::create_comment,
    delete::delete_comment,
    read::get_comment,
    remove::remove_comment,
    update::edit_comment,
  },
  community::{
    create::create_community,
    delete::delete_community,
    list::list_communities,
    remove::remove_community,
    update::edit_community,
  },
  custom_emoji::{
    create::create_custom_emoji,
    delete::delete_custom_emoji,
    list::list_custom_emojis,
    update::edit_custom_emoji,
  },
  invite::{create::create_invitation, list::list_invitations, revoke::revoke_invitation},
  multi_community::{
    create::create_multi_community,
    create_entry::create_multi_community_entry,
    delete_entry::delete_multi_community_entry,
    list::list_multi_communities,
    update::edit_multi_community,
  },
  oauth_provider::{
    create::create_oauth_provider,
    delete::delete_oauth_provider,
    update::edit_oauth_provider,
  },
  post::{
    create::create_post,
    delete::delete_post,
    read::get_post,
    remove::remove_post,
    update::edit_post,
  },
  private_message::{
    create::create_private_message,
    delete::delete_private_message,
    update::edit_private_message,
  },
  site::{create::create_site, read::get_site, update::edit_site},
  tagline::{
    create::create_tagline,
    delete::delete_tagline,
    list::list_taglines,
    update::edit_tagline,
  },
  user::{
    create::{authenticate_with_oauth, register},
    delete::delete_account,
    my_user::get_my_user,
  },
};
use lemmy_routes::images::{
  delete::{
    delete_community_banner,
    delete_community_icon,
    delete_image,
    delete_image_admin,
    delete_site_banner,
    delete_site_icon,
    delete_user_avatar,
    delete_user_banner,
  },
  download::{get_image, image_proxy},
  pictrs_health,
  upload::{
    upload_community_banner,
    upload_community_icon,
    upload_image,
    upload_site_banner,
    upload_site_icon,
    upload_user_avatar,
  },
};
use lemmy_utils::rate_limit::RateLimit;

pub fn config(cfg: &mut ServiceConfig, rate_limit: &RateLimit) {
  cfg.service(
    scope("/api/v4")
      .wrap(rate_limit.message())
      // Site
      .service(
        scope("/site")
          .route("", get().to(get_site))
          .route("", post().to(create_site))
          .route("", put().to(edit_site))
          .route("/icon", post().to(upload_site_icon))
          .route("/icon", delete().to(delete_site_icon))
          .route("/banner", post().to(upload_site_banner))
          .route("/banner", delete().to(delete_site_banner)),
      )
      .route("/modlog", get().to(get_mod_log))
      .service(
        resource("/search")
          .wrap(rate_limit.search())
          .route(get().to(search)),
      )
      .service(
        resource("/resolve_object")
          .wrap(rate_limit.search())
          .route(get().to(resolve_object)),
      )
      // Community
      .service(
        resource("/community")
          .guard(guard::Post())
          .wrap(rate_limit.register())
          .route(post().to(create_community)),
      )
      .service(
        scope("/community")
          .route("", get().to(get_community))
          .route("", put().to(edit_community))
          .route("", delete().to(delete_community))
          .route("/random", get().to(get_random_community))
          .route("/list", get().to(list_communities))
          .route("/follow", post().to(follow_community))
          .route("/report", post().to(create_community_report))
          .route("/report/resolve", put().to(resolve_community_report))
          // Mod Actions
          .route("/remove", post().to(remove_community))
          .route("/transfer", post().to(transfer_community))
          .route("/ban_user", post().to(ban_from_community))
          .route("/mod", post().to(add_mod_to_community))
          .route("/icon", post().to(upload_community_icon))
          .route("/icon", delete().to(delete_community_icon))
          .route("/banner", post().to(upload_community_banner))
          .route("/banner", delete().to(delete_community_banner))
          .route("/tag", post().to(create_community_tag))
          .route("/tag", put().to(edit_community_tag))
          .route("/tag", delete().to(delete_community_tag))
          .route("/notifications", put().to(edit_community_notifications))
          .service(
            scope("/pending_follows")
              .route("/list", get().to(get_pending_follows_list))
              .route("/approve", post().to(post_pending_follows_approve)),
          ),
      )
      .service(
        scope("/multi_community")
          .route("", post().to(create_multi_community))
          .route("", put().to(edit_multi_community))
          .route("", get().to(read_multi_community))
          .route("/entry", post().to(create_multi_community_entry))
          .route("/entry", delete().to(delete_multi_community_entry))
          .route("/list", get().to(list_multi_communities))
          .route("/follow", post().to(follow_multi_community)),
      )
      .route("/federated_instances", get().to(get_federated_instances))
      // Post
      .service(
        resource("/post")
          // Handle POST to /post separately to add the post() rate limiter
          .guard(guard::Post())
          .wrap(rate_limit.post())
          .route(post().to(create_post)),
      )
      .service(
        resource("/post/site_metadata")
          .wrap(rate_limit.search())
          .route(get().to(get_link_metadata)),
      )
      .service(
        scope("/post")
          .route("", get().to(get_post))
          .route("", put().to(edit_post))
          .route("", delete().to(delete_post))
          .route("/remove", post().to(remove_post))
          .route("/mark_as_read", post().to(mark_post_as_read))
          .route("/mark_as_read/many", post().to(mark_posts_as_read))
          .route("/hide", post().to(hide_post))
          .route("/lock", post().to(lock_post))
          .route("/feature", post().to(feature_post))
          .route("/list", get().to(list_posts))
          .route("/like", post().to(like_post))
          .route("/like/list", get().to(list_post_likes))
          .route("/save", put().to(save_post))
          .route("/report", post().to(create_post_report))
          .route("/report/resolve", put().to(resolve_post_report))
          .route("/notifications", put().to(edit_post_notifications))
          .route("/mod_edit", put().to(mod_edit_post))
          .route("/warn", post().to(create_post_warning)),
      )
      // Comment
      .service(
        // Handle POST to /comment separately to add the comment() rate limiter
        resource("/comment")
          .guard(guard::Post())
          .wrap(rate_limit.comment())
          .route(post().to(create_comment)),
      )
      .service(
        scope("/comment")
          .route("", get().to(get_comment))
          .route("", put().to(edit_comment))
          .route("", delete().to(delete_comment))
          .route("/remove", post().to(remove_comment))
          .route("/distinguish", post().to(distinguish_comment))
          .route("/like", post().to(like_comment))
          .route("/like/list", get().to(list_comment_likes))
          .route("/save", put().to(save_comment))
          .route("/lock", post().to(lock_comment))
          .route("/list", get().to(list_comments))
          .route("/list/slim", get().to(list_comments_slim))
          .route("/warn", post().to(create_comment_warning))
          .route("/report", post().to(create_comment_report))
          .route("/report/resolve", put().to(resolve_comment_report)),
      )
      // Private Message
      .service(
        scope("/private_message")
          .route("", post().to(create_private_message))
          .route("", put().to(edit_private_message))
          .route("", delete().to(delete_private_message))
          .route("/report", post().to(create_pm_report))
          .route("/report/resolve", put().to(resolve_pm_report)),
      )
      // Reports
      .service(
        scope("/report")
          .wrap(rate_limit.message())
          .route("/list", get().to(list_reports)),
      )
      // User
      .service(
        scope("/account/auth")
          .wrap(rate_limit.register())
          .route("/register", post().to(register))
          .route("/login", post().to(login))
          .route("/logout", post().to(logout))
          .route("/password_reset", post().to(reset_password))
          .route("/password_change", post().to(change_password_after_reset))
          .route("/change_password", put().to(change_password))
          .route("/totp/generate", post().to(generate_totp_secret))
          .route("/totp/edit", post().to(edit_totp))
          .route("/verify_email", post().to(verify_email))
          .route(
            "/resend_verification_email",
            post().to(resend_verification_email),
          ),
      )
      .service(
        scope("/account")
          .route("/auth/get_captcha", get().to(get_captcha))
          .route("", get().to(get_my_user))
          .route("/unread_counts", get().to(get_unread_counts))
          .service(
            scope("/media")
              .route("", delete().to(delete_image))
              .route("/list", get().to(list_media)),
          )
          .service(
            scope("/notification")
              .route("/list", get().to(list_notifications))
              .route("/mark_as_read/all", post().to(mark_all_notifications_read))
              .route("/mark_as_read", post().to(mark_notification_as_read)),
          )
          .route("", delete().to(delete_account))
          .route("/login/list", get().to(list_logins))
          .route("/validate_auth", get().to(validate_auth))
          .route(
            "/donation_dialog_shown",
            post().to(mark_donation_dialog_shown),
          )
          .route("/avatar", post().to(upload_user_avatar))
          .route("/avatar", delete().to(delete_user_avatar))
          // audioprn: banners for verified creators and admins only
          .route("/banner", post().to(lemmy_audioprn::upload_user_banner))
          .route("/banner", delete().to(delete_user_banner))
          .service(
            scope("/block")
              .route("/person", post().to(user_block_person))
              .route("/community", post().to(user_block_community))
              .route(
                "/instance/communities",
                post().to(user_block_instance_communities),
              )
              .route("/instance/persons", post().to(user_block_instance_persons)),
          )
          .route("/saved", get().to(list_person_saved))
          .route("/read", get().to(list_person_read))
          .route("/hidden", get().to(list_person_hidden))
          .route("/liked", get().to(list_person_liked))
          .route("/settings/save", put().to(save_user_settings))
          // Account settings import / export have a strict rate limit
          .service(
            scope("/settings")
              .wrap(rate_limit.import_user_settings())
              .route("/export", get().to(export_user_settings))
              .route("/import", post().to(import_user_settings)),
          )
          .service(
            resource("/data/export")
              .wrap(rate_limit.import_user_settings())
              .route(get().to(export_user_data)),
          )
          .service(
            scope("/invite")
              .route("", post().to(create_invitation))
              .route("", delete().to(revoke_invitation))
              .route("/list", get().to(list_invitations)),
          ),
      )
      // audioprn: Reddit takedown requests, open to everyone, so rate limited like signups
      .service(
        resource("/audioprn/takedown_request")
          .guard(guard::Post())
          .wrap(rate_limit.register())
          .route(post().to(lemmy_audioprn::optout::submit_takedown_request)),
      )
      // audioprn: verified creators, post credits and reactions
      .service(
        scope("/audioprn")
          .route(
            "/creator_application",
            post().to(lemmy_audioprn::submit_creator_application),
          )
          .route(
            "/creator_application/mine",
            get().to(lemmy_audioprn::get_my_creator_application),
          )
          .route(
            "/creator_application/list",
            get().to(lemmy_audioprn::list_creator_applications),
          )
          .route(
            "/creator_application/approve",
            put().to(lemmy_audioprn::approve_creator_application),
          )
          .route("/post_credits", get().to(lemmy_audioprn::post::get_post_credits))
          .route("/post_credits", put().to(lemmy_audioprn::post::set_post_credits))
          .route(
            "/post_reactions",
            get().to(lemmy_audioprn::reaction::get_post_reactions),
          )
          .route(
            "/post_reaction",
            put().to(lemmy_audioprn::reaction::set_post_reaction),
          )
          .route(
            "/post_favourite",
            put().to(lemmy_audioprn::reaction::set_post_favourite),
          )
          .route(
            "/verified_creators",
            get().to(lemmy_audioprn::list_verified_creators),
          )
          .route("/creators", get().to(lemmy_audioprn::creators::list_creators))
          .route(
            "/creator_followers",
            get().to(lemmy_audioprn::creators::my_followers),
          )
          .route(
            "/creator_settings",
            put().to(lemmy_audioprn::creators::set_creator_settings),
          )
          .route("/news", get().to(lemmy_audioprn::news::list_news))
          .route("/news", post().to(lemmy_audioprn::news::create_news))
          .route("/news", delete().to(lemmy_audioprn::news::delete_news))
          .route("/news/main", put().to(lemmy_audioprn::news::set_news_in_main))
          .route("/post_script", get().to(lemmy_audioprn::script::get_post_script))
          .route("/post_script", put().to(lemmy_audioprn::script::set_post_script))
          .route(
            "/scriptbin_posts",
            get().to(lemmy_audioprn::script::list_scriptbin_posts),
          )
          .route("/script_watermark", post().to(lemmy_audioprn::script::check_watermark))
          .route("/creator_pin", get().to(lemmy_audioprn::creator_page::get_creator_pin))
          .route("/creator_pin", put().to(lemmy_audioprn::creator_page::set_creator_pin))
          .route("/creator_stats", get().to(lemmy_audioprn::creator_page::get_creator_stats))
          .route("/creator_page_pins", get().to(lemmy_audioprn::creator_page::get_page_pins))
          .route("/creator_page_pin", put().to(lemmy_audioprn::creator_page::set_page_pin))
          .route("/blocks", get().to(lemmy_audioprn::blocks::get_blocks))
          .route("/popular_tags", get().to(lemmy_audioprn::search::get_popular_tags))
          .route("/online", get().to(lemmy_audioprn::online::get_online))
          .route("/search_prefs", get().to(lemmy_audioprn::search::get_search_prefs))
          .route("/search_prefs", put().to(lemmy_audioprn::search::set_search_prefs))
          .route("/script_fill", get().to(lemmy_audioprn::search::get_script_fill))
          .route("/script_fill", put().to(lemmy_audioprn::search::set_script_fill))
          .route("/post_drafts", get().to(lemmy_audioprn::drafts::list_drafts))
          .route("/post_draft", get().to(lemmy_audioprn::drafts::get_draft))
          .route("/post_draft", put().to(lemmy_audioprn::drafts::save_draft))
          .route("/post_draft", delete().to(lemmy_audioprn::drafts::delete_draft))
          .route(
            "/reddit_import",
            post().to(lemmy_api_crud::post::reddit_import::import_reddit_post),
          )
          .route("/reddit_imports", get().to(lemmy_audioprn::reddit::list_imports))
          // A whole masterlist in one reply, so it shares the search rate limit.
          .service(
            resource("/masterlist")
              .wrap(rate_limit.search())
              .route(get().to(lemmy_audioprn::masterlist::get_masterlist)),
          )
          .route(
            "/masterlist/categories",
            put().to(lemmy_audioprn::masterlist::set_categories),
          )
          .route("/masterlist/entry", put().to(lemmy_audioprn::masterlist::save_entry))
          .route(
            "/masterlist/import",
            put().to(lemmy_audioprn::masterlist::import_entries),
          )
          .route(
            "/masterlist/favourite",
            put().to(lemmy_audioprn::masterlist::set_favourite),
          )
          .route(
            "/masterlist/entry",
            delete().to(lemmy_audioprn::masterlist::delete_entry),
          )
          .route("/taboo_tags", get().to(lemmy_audioprn::post::get_taboo_tags))
          .route(
            "/credited_posts",
            get().to(lemmy_audioprn::post::get_credited_posts),
          )
          .route(
            "/credited_posts/hide",
            put().to(lemmy_audioprn::post::hide_credited_post),
          )
          .route("/taboo_tags", put().to(lemmy_audioprn::post::set_taboo_tags))
          .route(
            "/creator_footers",
            get().to(lemmy_audioprn::creators::get_creator_footers),
          )
          .route(
            "/creator_footer",
            put().to(lemmy_audioprn::creators::set_creator_footer),
          )
          .route("/post_poll", put().to(lemmy_audioprn::poll::create_post_poll))
          .route("/post_polls", get().to(lemmy_audioprn::poll::get_post_polls))
          .route(
            "/post_poll/vote",
            post().to(lemmy_audioprn::poll::vote_post_poll),
          )
          .route("/awards", get().to(lemmy_audioprn::awards::get_awards))
          .route("/post_award", put().to(lemmy_audioprn::awards::give_post_award))
          .route("/award_types", get().to(lemmy_audioprn::awards::get_award_types))
          .route("/award_type", post().to(lemmy_audioprn::awards::create_award_type))
          .route("/award_type", put().to(lemmy_audioprn::awards::edit_award_type))
          .route("/award_settings", get().to(lemmy_audioprn::awards::get_award_settings))
          .route("/award_settings", put().to(lemmy_audioprn::awards::set_award_settings))
          .route("/opt_outs", get().to(lemmy_audioprn::optout::list_opt_outs))
          .route("/opt_out", post().to(lemmy_audioprn::optout::add_opt_out))
          .route("/opt_out", delete().to(lemmy_audioprn::optout::delete_opt_out))
          .route(
            "/takedown_request",
            delete().to(lemmy_audioprn::optout::dismiss_takedown_request),
          )
          .route("/feed_prefs", get().to(lemmy_audioprn::feed::get_feed_prefs))
          .route("/feed_prefs", put().to(lemmy_audioprn::feed::set_feed_prefs))
          .route(
            "/profile_comment/remove",
            put().to(lemmy_audioprn::feed::remove_profile_comment),
          )
          .route(
            "/creator_follow",
            put().to(lemmy_audioprn::creators::follow_creator),
          ),
      )
      // Person / User actions
      .service(
        scope("/person")
          .route("", get().to(read_person))
          .route("/list", get().to(list_persons))
          .route("/content", get().to(list_person_content))
          .route("/note", post().to(user_note_person)),
      )
      // Admin Actions
      .service(
        scope("/admin")
          .route("/add", post().to(add_admin))
          .service(
            scope("/registration_application")
              .route("", get().to(get_registration_application))
              .route("/list", get().to(list_registration_applications))
              .route("/approve", put().to(approve_registration_application)),
          )
          .service(
            scope("/purge")
              .route("/person", post().to(purge_person))
              .route("/community", post().to(purge_community))
              .route("/post", post().to(purge_post))
              .route("/comment", post().to(purge_comment)),
          )
          .service(
            scope("/tagline")
              .route("", post().to(create_tagline))
              .route("", put().to(edit_tagline))
              .route("", delete().to(delete_tagline))
              .route("/list", get().to(list_taglines)),
          )
          .route("/ban", post().to(ban_from_site))
          .route("/users", get().to(admin_list_users))
          .service(
            scope("/instance")
              .route("/block", post().to(admin_block_instance))
              .route("/allow", post().to(admin_allow_instance)),
          ),
      )
      .service(
        scope("/custom_emoji")
          .route("", post().to(create_custom_emoji))
          .route("", put().to(edit_custom_emoji))
          .route("", delete().to(delete_custom_emoji))
          .route("/list", get().to(list_custom_emojis)),
      )
      .service(
        scope("/oauth_provider")
          .route("", post().to(create_oauth_provider))
          .route("", put().to(edit_oauth_provider))
          .route("", delete().to(delete_oauth_provider)),
      )
      .service(
        scope("/oauth")
          .wrap(rate_limit.register())
          .route("/authenticate", post().to(authenticate_with_oauth)),
      )
      .service(
        scope("/image")
          .service(
            resource("")
              .wrap(rate_limit.image())
              .route(post().to(upload_image))
              .route(delete().to(delete_image_admin)),
          )
          .route("/proxy", get().to(image_proxy))
          .route("/health", get().to(pictrs_health))
          .route("/list", get().to(list_all_media))
          .route("/{filename}", get().to(get_image)),
      ),
  );
}
