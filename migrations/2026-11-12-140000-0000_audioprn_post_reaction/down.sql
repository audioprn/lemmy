DROP TRIGGER audioprn_post_reaction_changed ON post_reaction;

DROP TRIGGER audioprn_post_engagement ON post;

DROP FUNCTION audioprn_post_reaction_changed, audioprn_post_engagement;

DROP TABLE post_reaction;

UPDATE
    post
SET
    score = upvotes - downvotes;
