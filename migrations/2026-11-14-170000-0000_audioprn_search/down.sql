DROP TABLE search_prefs;

DROP TRIGGER audioprn_detect_fill ON reddit_import;

DROP TRIGGER audioprn_detect_fill ON post_community_tag;

DROP TRIGGER audioprn_detect_fill ON post;

DROP FUNCTION audioprn_detect_fill_reddit_trigger, audioprn_detect_fill_flair_trigger, audioprn_detect_fill_post_trigger, audioprn_detect_fill, audioprn_is_script_offer;

DROP TABLE script_fill;

DROP FUNCTION audioprn_post_length, audioprn_minutes, audioprn_regex_match, audioprn_like;

