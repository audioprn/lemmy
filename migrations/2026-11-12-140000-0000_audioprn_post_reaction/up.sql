-- audioprn: one emoji reaction per user per post, instead of post votes.
CREATE TABLE post_reaction (
    post_id int NOT NULL REFERENCES post ON UPDATE CASCADE ON DELETE CASCADE,
    person_id int NOT NULL REFERENCES person ON UPDATE CASCADE ON DELETE CASCADE,
    emoji text NOT NULL CHECK (emoji IN ('fire', 'gem', 'black_heart', 'pleading', 'droplets')),
    published_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (post_id, person_id)
);

-- Engagement = reactions + 5 × comments. It's kept in post.score (post votes are off), so
-- Lemmy's Top sort, shown as "Engagement", sorts by it, with its time ranges and paging.
CREATE FUNCTION audioprn_post_engagement ()
    RETURNS TRIGGER
    LANGUAGE plpgsql
    AS $$
BEGIN
    NEW.score := 5 * NEW.comments + (
        SELECT
            count(*)
        FROM
            post_reaction
        WHERE
            post_id = NEW.id);
    RETURN NEW;
END
$$;

CREATE TRIGGER audioprn_post_engagement
    BEFORE INSERT OR UPDATE ON post
    FOR EACH ROW
    EXECUTE FUNCTION audioprn_post_engagement ();

-- Any update recomputes the score (see above).
CREATE FUNCTION audioprn_post_reaction_changed ()
    RETURNS TRIGGER
    LANGUAGE plpgsql
    AS $$
BEGIN
    UPDATE
        post
    SET
        score = 0
    WHERE
        id = coalesce(NEW.post_id, OLD.post_id);
    RETURN NULL;
END
$$;

CREATE TRIGGER audioprn_post_reaction_changed
    AFTER INSERT OR DELETE ON post_reaction
    FOR EACH ROW
    EXECUTE FUNCTION audioprn_post_reaction_changed ();

UPDATE
    post
SET
    score = 0;
